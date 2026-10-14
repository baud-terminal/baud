//! The window, the responder, and the frame.
#![forbid(unsafe_code)]

use crate::bell::{Bell, Rings, Update, UpdateClock};
use crate::caret::{Caret, Cycle, Gate, NonBlinking, Owes};
use crate::host::{
    FIRST_BYTES, FIRST_COMMIT, FIRST_CONTENT, FIRST_DONE, FIRST_PRESENT, FIRST_TICK,
    FIRST_TICK_DONE, MAX_GRID, cell_px, describe, host_geometry, load_faces, startup_first,
    startup_mark, startup_note,
};
use crate::latency::{Ended, Judged, Latency, Switch};
use crate::pty::{Outbox, Pty, Stream, Wrote, host_now};
use crate::reveal::{Button as BarButton, Drag, Grab, Knob, Motion, Over, Reveal, Scale, Takes};
use crate::wheel::{Delta, Route, Travel};
use baud_core::{Depth, Erasure, Owned, Screen, Term, Zone, grid_stride};
use baud_domain::{
    CellFlags, Codepoint, Col, Cols, Content, FnKey, HostTime, ImagePx, KeyCode, KeyEvent,
    KeyEventKind, Modifiers, MouseButton, MouseEvent, MouseKind, MouseMods, MouseTracking,
    PackedCell, PointerPx, Row, RowRot, Rows, WinSize,
};
use baud_font::Px;
use baud_present_macos::clipboard;
use baud_present_macos::{
    CellInput, Drawn, Gpu, GpuFrame, GridBuffer, InFlight, MapBuffer, Mode, OverlayBuffer,
    Presented, Presenter, StylesBuffer, SurfaceSpace,
    journal::{Reshape, Wheel},
};
use baud_render::Place;
use baud_render::{
    AbsLine, Blend, Blink, BlinkPhases, Blinking, Candidate, Clip, Composition, Damage, Frame,
    GlyphMap, Grain, Highlight, Instance, Look, MapEntry, Overlay, Point, Press, PxSize, Renderer,
    Resolved, Scene, Scroll as ScrollBar, Scrolled, Scroller, Selection, Slide, Span, StyleStamp,
    Tally, Target, Theme, Trail, TrailLook, Transition, Window, back_for, gutter, place, thumb,
};
use macos_liquid_glass::drawable::{
    Appearance, Button as ViewButton, DrawableView, Focus, Handled, KeyPress, Mouse,
    MouseKind as ViewMouseKind, Rect, Responder, Scroll, Waker,
};
use macos_liquid_glass::glass::{GlassError, GlassStyle, GlassSurface};
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::{MainThreadOnly, Message};
use macos_liquid_glass::objc2_app_kit::{
    NSAppearanceCustomization, NSApplication, NSApplicationActivationPolicy,
    NSAutoresizingMaskOptions, NSBeep, NSColor, NSImage, NSImageView, NSRequestUserAttentionType,
    NSScreen, NSScroller, NSScrollerStyle, NSView, NSWindow, NSWindowButton, NSWindowStyleMask,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{
    MainThreadMarker, NSBundle, NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSRect, NSSize,
    NSString, NSUserDefaults,
};
use macos_liquid_glass::window::GlassWindow;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

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
/// Half a slow blink cycle, milliseconds: text with `SGR 5` shows for this
/// long, then hides for this long -- 75 blinks a minute. `[[source: ecma48]]`
/// § 8.3.117 gives 5 as "slowly blinking (less then 150 per minute)".
const BLINK_HALF_MS: u32 = 400;
/// Half a rapid blink cycle, milliseconds: text with `SGR 6` shows for this
/// long, then hides for this long -- 150 blinks a minute, 2.5 a second, the
/// floor of § 8.3.117's "rapidly blinking (150 per minute or more)", which
/// the owner chose on 2026-10-13. Under WCAG 2.3.1's "more than three times
/// in any one second period" (`[[source: wcag22]]`), whose flash is "a pair
/// of opposing changes": one shown-and-hidden cycle is one flash.
const RAPID_HALF_MS: u32 = 200;
// The two bounds, in integers: 150 a minute is exactly the rapid floor, so a
// float comparison would sit on a rounding edge.
const _: () = assert!(
    60_000 < 150 * 2 * BLINK_HALF_MS,
    "SGR 5 is slow blink: fewer than 150 a minute"
);
const _: () = assert!(
    60_000 >= 150 * 2 * RAPID_HALF_MS,
    "SGR 6 is rapid blink: 150 a minute or more"
);
/// The two halves in seconds, the display link's unit.
const BLINK_HALF: f64 = BLINK_HALF_MS as f64 / 1000.0;
const RAPID_HALF: f64 = RAPID_HALF_MS as f64 / 1000.0;

/// The phases blinking text draws with at display-link time `target`, one a
/// rate: the clock's, unless the system asks for reduced motion, when both
/// are held shown. Apple's HIG names blinking under Reduce Motion, and WCAG
/// 2.2.2 asks for a way to stop blinking that lasts past five seconds; this
/// is that way, and the reasons are on
/// `reduce_motion_holds_blinking_text_shown`. Held, no phase flips, so a
/// blinking cell owes no frame. Each rate shows for half its cycle, as
/// `SGR 5` always did: the text gives a rate and no duty cycle.
fn text_blink(target: f64, motion: crate::config::SystemMotion) -> BlinkPhases {
    use crate::config::SystemMotion;
    let at = |half: f64| match (target / half).rem_euclid(2.0) < 1.0 {
        true => Blink::Shown,
        false => Blink::Hidden,
    };
    match motion {
        SystemMotion::Reduced => BlinkPhases::SHOWN,
        SystemMotion::Full => BlinkPhases {
            slow: at(BLINK_HALF),
            rapid: at(RAPID_HALF),
        },
    }
}
/// How long a cursor trail's ghost takes to sweep from the old cell to the
/// new, seconds -- short and snappy, and only paid when the trail is on.
const TRAIL_S: f64 = 0.1;
/// How long the alternate-screen fade takes to reveal the new screen,
/// seconds. Only paid on a screen switch when the transition is on.
const TRANSITION_S: f64 = 0.15;

/// An alternate-screen switch being animated: the display-link timestamp
/// the fade began at. `progress` is `(now - start) / TRANSITION_S`, off the
/// same clock as the blink and the trail.
#[derive(Clone, Copy)]
struct TransitionState {
    start: f64,
}

/// A cursor move being animated: the cell the cursor left, the cell it
/// reached, and the display-link timestamp the sweep began at. `progress`
/// is `(now - start) / TRAIL_S`, computed off the tick's own clock the way
/// the blink phase is, so the renderer only interpolates.
#[derive(Clone, Copy, PartialEq, Debug)]
struct TrailState {
    from: (Row, Col),
    to: (Row, Col),
    start: f64,
    /// How far the cell it left has ridden a smooth scroll, pixels, y down
    /// (`ride_ghost`).
    origin_dy: f32,
    /// How far the ghost itself has ridden, and whether it has stopped.
    ride: GhostRide,
}

/// **How far a ghost has ridden a smooth scroll**, pixels, y down: at most
/// [`crate::trail::THRESHOLD`] rows, then it stops riding and finishes its
/// sweep to the cursor -- the quad's rule, where a move past the bound folds
/// in (`Chase::ride`).
#[derive(Clone, Copy, PartialEq, Debug)]
enum GhostRide {
    Riding(f32),
    Folded,
}

/// Which path draws: the CPU compositor, Metal over the converted
/// instances, or Metal over the raw grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Path {
    Cpu,
    Instances,
    Cells,
}

/// The trail the driver runs on `path`: an effect is the GPU's, so on the
/// CPU path it rides the quad and runs as one -- no frames are owed for an
/// effect nothing draws.
fn chase_style(style: crate::trail::Style, path: Path) -> crate::trail::Style {
    match (style, path) {
        (crate::trail::Style::Effect(_), Path::Cpu) => {
            crate::trail::Style::Smear(baud_render::Shape::Quad)
        }
        (style, _) => style,
    }
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
    candidates: Vec<Candidate>,
    /// **The core's own grid, held and no longer bound.**
    ///
    /// The core's storage is lent out of these two Metal allocations
    /// (`Owned::storage_over`), so dropping either would free memory the
    /// parser is writing into -- they are an ownership role now rather
    /// than a binding. Nothing reads them on the GPU since D4: the shader
    /// reads [`CellMemory::copies`], and which surface is up is resolved
    /// when the window is copied rather than by rebinding.
    #[expect(
        dead_code,
        reason = "keeps alive the allocation the core's grid is lent from"
    )]
    grid: GridBuffer,
    #[expect(
        dead_code,
        reason = "keeps alive the allocation the alternate screen is lent from"
    )]
    alt: GridBuffer,
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
    /// `rows`. D4 prices the copy at 1.2 us against a 16,667 us frame, and
    /// `frame_gpu`'s `copied` arm measured it *faster* end to end than the
    /// in-place read: 30-60 us less GPU time for about one microsecond of
    /// CPU.
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
/// or a wake from another thread.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Trigger {
    Tick,
    Wake,
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
/// terminal has. **The window over the history goes to the live screen**:
/// a program that changes the page width is redrawing it.
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
    Some(crate::width::Grid::of(term))
}

/// `OSC 8` activation, for the five-second report: links opened, revealed
/// in Finder, and refused -- each refusal also a `warn!` naming the rule,
/// the `mouse_unencodable` shape of refusing and saying how often.
#[derive(Clone, Copy, Default, Debug)]
struct Links {
    opened: Tally,
    revealed: Tally,
    refused: Tally,
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
    /// What the child is owed and the kernel has not taken (`pty.rs`):
    /// every write goes through it, in order, and the pump offers what is
    /// left on every turn -- the display link's tick among them -- so a
    /// queue that fills is retried on the clock the loop already has, and
    /// the window never waits in `write(2)` for a child that is not reading.
    outbox: Outbox,
    frame: Vec<u32>,
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
    /// How many lines above the live screen the window sits: zero is the
    /// live screen, and the bound is what the core retains. Anchored to
    /// content while output arrives, so the line being read stays put;
    /// a key press snaps it back to the bottom.
    back: Rows,
    /// The selection, over absolute lines of the live surface -- see
    /// `docs/research/notes/k-selection.md` -- and the rotation's advance
    /// since launch that numbers those lines.
    selection: Option<Selection>,
    scrolled: Scrolled,
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
    /// The scroller "Show scroll bars" asked for when the window was last
    /// laid out: under the legacy style the grid gives up a gutter to it.
    scroller: Scroller,
    /// Wheel travel not yet a whole line (`wheel.rs`).
    travel: Travel,
    frames: Tally,
    /// Display-link ticks, which `frames` is not: a tick that finds
    /// nothing pending returns from `present` before a frame is built,
    /// so on an idle screen `frames` stops and this does not. Row 11's
    /// metric is what those ticks cost, and it needs both numbers.
    ticks: Tally,
    /// Frames whose shared-buffer writes began while the GPU was still
    /// reading the last one's. See the comment at the count site.
    wrote_in_flight: Tally,
    links: Links,
    /// See [`Trace`].
    trace: Option<Trace>,
    last_report: std::time::Instant,
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
    last_zone: Zone,
    last_tick: Option<f64>,
    /// When the synchronised update (`?2026`) open now opened, timed from its
    /// own opening. A frame is held while one is open, so a half-drawn screen --
    /// btop's menu redraws inside one -- is never presented; but only up to
    /// [`SYNC_MAX`], so a program that opens one and stalls -- or dies inside
    /// it, leaving it open until a `?2026l`, `DECSTR` or `RIS` -- cannot
    /// freeze the window.
    updates: UpdateClock<Instant>,
    /// The blink phase this frame draws with, whether any cell blinks, and
    /// whether the phase just flipped -- which forces a full repaint, since a
    /// blink changes no cell content and row damage would not catch it.
    blink: BlinkPhases,
    blink_present: Blinking,
    /// The cursor's blink, on the display link's clock (`caret.rs`): the
    /// system caret's rate, read on the first tick and at every focus gain,
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
    trail: Option<TrailState>,
    /// The quad or sweep trail's state between frames (`trail.rs`).
    chase: crate::trail::Chase<HostTime>,
    last_cursor_cell: Option<(Row, Col)>,
    tick_target: f64,
    /// Whether an alternate-screen switch fades, from `[screen] transition`,
    /// and the fade being animated. `Some` forces a frame the way a trail
    /// does, so an idle terminal on either screen builds nothing.
    screen_transition: crate::config::ScreenTransition,
    transition: Option<TransitionState>,
    /// Whether output scrolls smoothly, from `[scroll] mode`, and the live
    /// slide it drives. The slide's offset is capped at one row, so a fast
    /// burst never accumulates lag; a non-zero pan forces a frame the way a
    /// trail does, and an idle screen -- whose pan is zero -- forces none.
    scroll: crate::config::ScrollMode,
    /// Whether a trail in flight rides a smooth scroll, from `[cursor]
    /// trail_scroll`.
    trail_scroll: crate::config::TrailScroll,
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
}

/// What launch prepares before a window or a grid exists -- the cache
/// geometry, the GPU, the config, the faces and the renderer -- so the
/// child shell can be spawned and AppKit started with all of it done.
/// Under six milliseconds together; the point is what it lets overlap.
struct Boot {
    request: Request,
    geom: baud_domain::CacheGeometry,
    pitch: baud_domain::Stride,
    probe: std::thread::JoinHandle<Option<Gpu>>,
    renderer: Renderer<'static, 'static>,
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
    /// Whether an alternate-screen switch fades, from `[screen] transition`.
    screen_transition: crate::config::ScreenTransition,
    /// Whether output scrolls smoothly, from `[scroll] mode`.
    scroll: crate::config::ScrollMode,
    /// Whether a trail in flight rides a smooth scroll, from `[cursor]
    /// trail_scroll`.
    trail_scroll: crate::config::TrailScroll,
    /// How much caller memory the image store is given, from `[images]`.
    images: baud_domain::ImageBudget,
    /// Whether the DA claims Sixel, from `[images] advertise_sixel`.
    sixel: baud_domain::SixelClaim,
    /// Whether a `U+FE0F` widens its base, from `vs16_width`.
    vs16_width: baud_domain::Vs16Width,
    /// `[clipboard]`.
    clipboard: crate::config::Clipboard,
    /// What a `BEL` does in a focused window, from `bell`.
    bell: crate::config::BellStyle,
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
        // process: the renderer borrows them for the life of the window. On
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
        // The Metal device on its own thread: creating it is 10-30 ms on
        // this box, and AppKit's initialisation on the main thread is what
        // it overlaps -- the probe is joined once the window exists.
        let probe = std::thread::spawn(move || {
            let t0 = Instant::now();
            let g = match request {
                Request::Force(Path::Cpu) => None,
                _ => Gpu::new(),
            };
            if let Some(g) = &g {
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
                // `scripts/shader-cache-bench.py` anchors a regex on the
                // whole line above -- appending to it would take the gate
                // with the finding.
                startup_note(&format!(
                    "gpu pipelines: bg {:.1} ms, glyph {:.1}, cell_bg {:.1}, cell_glyph {:.1}, cell_shape {:.1}",
                    b.each[0].as_secs_f64() * 1e3,
                    b.each[1].as_secs_f64() * 1e3,
                    b.each[2].as_secs_f64() * 1e3,
                    b.each[3].as_secs_f64() * 1e3,
                    b.each[4].as_secs_f64() * 1e3,
                ));
            }
            g
        });
        startup_mark("gpu probing");
        let cfg = crate::config::Config::load();
        startup_mark("config");
        match &cfg.source {
            Some(p) => crate::info!("config: {}", p.display()),
            None => crate::info!("config: none found; the built-in stack"),
        }
        for f in &cfg.faces {
            crate::info!("face: {}", f.path);
        }
        crate::info!("{}", crate::host::describe_images(&cfg.images));
        let faces = load_faces(&cfg);
        startup_mark("faces");
        // An explicit choice is the palette; `Auto` starts dark and is
        // corrected from the window's own appearance before the first
        // frame, because no window exists here to ask.
        // The theme files are read here, at launch, once: an appearance
        // flip swaps between two themes built from them and reads nothing.
        let (halves, said) = crate::palette::load_halves(
            cfg.palette_light.as_deref(),
            cfg.palette_dark.as_deref(),
            &crate::palette::Dirs::from_env(cfg.source.as_deref()),
        );
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
        let theme = theme_for(appearance, &cfg.glass, cfg.floor, &halves);
        let space = match cfg.colorspace {
            crate::config::Space::Srgb => SurfaceSpace::Srgb,
            crate::config::Space::DisplayP3 => SurfaceSpace::DisplayP3,
        };
        // At a placeholder grid: the renderer follows the terminal
        // (`Renderer::fit`) once the grid is known, and the cell metrics --
        // what the grid is computed from -- do not depend on it.
        let mut renderer = Renderer::new(
            crate::host::stack(&cfg, faces),
            PxSize::of(cfg.font_size * scale as f32).expect("size"),
            theme,
            Blend::Native,
            Cols::of_index(80),
            Rows::of_index(24),
        )
        .expect("cell metrics");
        renderer.set_emoji_default(cfg.emoji);
        startup_mark("renderer");
        Boot {
            request,
            geom,
            pitch,
            probe,
            renderer,
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
            screen_transition: cfg.screen_transition,
            scroll: cfg.scroll,
            trail_scroll: cfg.trail_scroll,
            images: cfg.images,
            sixel: cfg.sixel,
            vs16_width: cfg.vs16_width,
            clipboard: cfg.clipboard,
            bell: cfg.bell,
        }
    }

    fn metrics_cell(&self) -> (f64, f64) {
        let m = self.renderer.metrics();
        (f64::from(m.cell_w.value()), f64::from(m.cell_h.value()))
    }
}

impl App {
    fn new(ws: WinSize, sizing: Sizing, scale: CGFloat, waker: Waker, pty: Pty, boot: Boot) -> App {
        let Boot {
            request,
            geom,
            pitch,
            probe,
            mut renderer,
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
            screen_transition,
            scroll,
            trail_scroll,
            images,
            sixel,
            vs16_width,
            clipboard,
            bell: bell_style,
        } = boot;
        let probe = probe.join().unwrap_or(None);
        startup_mark("gpu joined");
        // The motion preference decides the trail before the chase is
        // built, so a launch under Reduce Motion never lights one. Read here,
        // after `sharedApplication`, which is what keeps AppKit's cached
        // answer invalidated.
        let motion_source = motion_override(std::env::var("BAUD_MOTION").ok().as_deref());
        let motion = system_motion(motion_source, reduce_motion);
        startup_mark("reduce motion");
        let trail_pick = cursor_trail;
        let cursor_trail = trail_pick.effective(motion);
        crate::info!("motion: {motion:?}; trail: {cursor_trail:?}");
        // The process's first scroller-style read is not the 5 ns a wheel
        // pays: AppKit loads the answer the first time, and this is where
        // the launch path makes it. Its own mark, so it is a number.
        let scroller = scroller_style();
        startup_mark("scroller style");
        let own_grid = |ws: &WinSize| {
            let owned: &'static mut Owned = Box::leak(Box::new(
                Owned::with_geometry(&MAX_GRID, &geom)
                    .with_images(images)
                    .with_clipboard(clipboard.budget),
            ));
            Term::new(owned.storage(), ws)
        };
        // **Both surfaces are the GPU's on the cell path.** The primary is
        // the ring -- the screen's capacity plus the scrollback -- and the
        // alternate screen is its own buffer of the capacity, so entering
        // it swaps which buffer the shader reads and copies nothing.
        let (mut wanted, gpu, term, grid) = match (request, probe) {
            (Request::Force(Path::Cells) | Request::Auto, Some(gpu)) => {
                let owned: &'static mut Owned = Box::leak(Box::new(
                    Owned::without_surfaces(&MAX_GRID, &geom, Depth::DEFAULT)
                        .with_images(images)
                        .with_clipboard(clipboard.budget),
                ));
                match (gpu.grid(owned.ring(), pitch), gpu.grid(owned.rows(), pitch)) {
                    (Some((grid, cells)), Some((alt, alt_cells))) => {
                        let term = Term::new(owned.storage_over(cells, alt_cells), &ws);
                        (Path::Cells, Some(gpu), term, Some((grid, alt)))
                    }
                    _ => {
                        crate::warn!("no shared grid; drawing the instances");
                        (Path::Instances, Some(gpu), own_grid(&ws), None)
                    }
                }
            }
            (Request::Force(Path::Instances), Some(gpu)) => {
                (Path::Instances, Some(gpu), own_grid(&ws), None)
            }
            (Request::Force(Path::Cells | Path::Instances) | Request::Auto, None) => {
                crate::warn!("no Metal device or shaders; drawing on the CPU");
                (Path::Cpu, None, own_grid(&ws), None)
            }
            (Request::Force(Path::Cpu), _) => (Path::Cpu, None, own_grid(&ws), None),
        };
        renderer.fit(&term);
        // The first frame is laid out for the scroller the setting asks for,
        // as every relayout after it is (`Driver::relayout`).
        renderer.set_gutter(gutter(scroller, crate::reveal::widths(Scale(scale))));
        let mut term = term;
        crate::host::tell_defaults(&mut term, renderer.theme(), baud_core::Overrides::Keep);
        let (frame_w, frame_h) = renderer.frame_size();
        let cell = match (&gpu, grid) {
            (Some(gpu), Some((grid, alt))) => match (sets(|| gpu.styles(&term)), gpu.map()) {
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
                        candidates: Vec::new(),
                        grid,
                        alt,
                        copies,
                        copies_mem,
                        style_stamps: vec![StyleStamp::default(); styles_mem.len()],
                        styles,
                        styles_mem,
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
        term.set_sixel_claim(sixel);
        term.set_vs16_width(vs16_width);
        term.set_initial_cursor_style(cursor);
        let bell = Bell::new(&term, crate::bell::LENGTH);
        let ringer = crate::bell::Ringer::new(&term);
        let clips = crate::clip::Clips::new(&term);
        // A read stops at a page-width request, and `pump` resizes the grid
        // before the rest of it is parsed (`width.rs`).
        term.set_width_hold(baud_core::WidthHold::On);
        let widths = crate::width::Widths::new(&term);
        App {
            bell,
            bell_style,
            ringer,
            term,
            renderer,
            presenter: None,
            gpu,
            cell,
            waker,
            pty,
            outbox: Outbox::default(),
            frame: vec![0; slots(frame_w) * slots(frame_h)],
            frame_w,
            frame_h,
            view_px: (frame_w, frame_h),
            pad_px: (Px::of(0), Px::of(0)),
            padding,
            scale,
            grid: ws,
            sizing,
            pending: Pending::Dirty,
            back: Rows::ZERO,
            selection: None,
            scrolled: Scrolled::START,
            reveal: Reveal::HIDDEN,
            drawn_look: Look::HIDDEN,
            pointer: Place::Grid,
            scroller,
            travel: Travel::default(),
            frames: Tally::default(),
            ticks: Tally::default(),
            wrote_in_flight: Tally::default(),
            links: Links::default(),
            trace,
            last_report: std::time::Instant::now(),
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
            last_zone: Zone::Output,
            last_tick: None,
            updates: UpdateClock::new(SYNC_MAX),
            blink: BlinkPhases::SHOWN,
            blink_present: Blinking::default(),
            // AppKit's rate until the first tick reads the system's; the
            // cursor is not drawn blinking before then, since focus starts
            // lost.
            caret: Caret::new(host_now(), Cycle::APPKIT, Gate::Free, cursor_stop),
            focus: baud_core::Focus::Lost,
            blink_choice,
            warned_blink: Done::No,
            caret_read: Done::No,
            next_damage: Damage::Incremental,
            cursor_trail,
            trail_pick,
            motion,
            motion_source,
            trail: None,
            chase: crate::trail::Chase::new(chase_style(cursor_trail.style(), wanted)),
            last_cursor_cell: None,
            tick_target: 0.0,
            screen_transition,
            transition: None,
            scroll,
            trail_scroll,
            slide: Slide::new(cell_h),
            latency: Latency::from_env(),
            images,
            clipboard,
            clips,
            widths,
        }
    }

    fn metrics_cell(&self) -> (f64, f64) {
        let m = self.renderer.metrics();
        (f64::from(m.cell_w.value()), f64::from(m.cell_h.value()))
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
        self.flush_reply();
    }

    /// Sends a key to the core and whatever it answers to the shell, and
    /// says whether anything did: a key the core encodes to nothing asks
    /// for no echo, and the instrument must not wait for one -- not even
    /// when the flush behind it writes bytes an earlier event left owed.
    fn send(&mut self, e: KeyEvent) -> Sent {
        // Into an empty buffer, so the key always fits and `false` is only a
        // key that encodes to nothing (`reads::input`).
        let encoded = crate::reads::input(&mut self.term, |t| t.key(e), |b| self.outbox.push(b));
        // A key shows the cursor and restarts its blink, whether or not the
        // child echoes it back, and is input to the stop (`caret.rs`). The
        // wheel's arrow keys on the alternate screen come through here too.
        self.caret.input(host_now());
        match (encoded, self.flush_reply()) {
            (true, Sent::Bytes) => Sent::Bytes,
            _ => Sent::Nothing,
        }
    }

    /// Where the window sits in the ring, for the scrollbar.
    fn window(&self) -> Window {
        window_of(&self.term, self.back)
    }

    /// How the thumb is seen at `now`: the reveal's under the overlay style,
    /// and constant under the legacy one, where nothing fades or hovers --
    /// so the tick, which forces a frame only when this changes, forces none
    /// for it, and a wheel's reveal draws nothing new.
    fn look(&self, now: HostTime) -> Look {
        self.reveal.look_under(self.scroller, now)
    }

    /// Whatever the core answered to the child, behind whatever it is
    /// already owed, as much as the kernel takes now; `Bytes` only when that
    /// emptied the outbox. With nothing owed it makes no system call: the
    /// pump runs this every tick.
    ///
    /// The reply was copied out and cleared whether or not the write went,
    /// so a short write lost the rest; the outbox keeps it.
    fn flush_reply(&mut self) -> Sent {
        self.outbox.push(self.term.reply());
        self.term.clear_reply();
        if self.outbox.is_empty() {
            return Sent::Nothing;
        }
        match self.pty.write(&mut self.outbox) {
            Wrote::All => Sent::Bytes,
            Wrote::Short => Sent::Nothing,
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
        // The surface is the renderer's size, so the renderer follows the
        // terminal first; the cell path never converts and would otherwise
        // keep the grid it started with.
        self.renderer.fit(&self.term);
        // The selection's share of this view, for every path's one rule --
        // or, while a program's `?1001` region tracks the mouse, that region
        // in its place (`tracked_highlight`).
        let highlight = {
            let back = looking(&self.term, self.back);
            let view = self.term.view(back);
            let top = self.scrolled.top(back);
            match self.term.highlight() {
                Some(_) => tracked_highlight(&self.term, self.scrolled, back),
                None => self
                    .selection
                    .and_then(|s| s.highlight(top, view.rows(), view.cols())),
            }
        };
        self.renderer.set_highlight(highlight);
        // The scrollbar's thumb, where the window sits in the ring, seen as
        // much as the reveal says at the display link's last target.
        let look = HostTime::of_secs(self.tick_target).map_or(Look::HIDDEN, |now| self.look(now));
        self.drawn_look = look;
        self.renderer.set_scrollbar(Some(ScrollBar {
            window: self.window(),
            look,
            widths: crate::reveal::widths(Scale(self.scale)),
            scroller: self.scroller,
        }));
        // The cursor trail, spawned here, where the post-feed cursor is
        // known and before any path converts; advanced and cleared off
        // `tick_target`, the display link's own clock, so the renderer only
        // draws what it is handed and the frame is a pure function of it.
        // The quad's rules are `trail.rs`'s; the ghost sweeps on any move.
        // The cursor it follows is `trail_cursor`'s: the one drawn, or where a
        // program placed a cursor it hid and draws its own.
        let followed = trail_cursor(&self.term, self.back);
        let cursor_cell = followed.map(TrailCursor::cell);
        if self.cursor_trail == crate::config::CursorTrail::Ghost {
            if let (Some(from), Some(to)) = (self.last_cursor_cell, cursor_cell) {
                if from != to {
                    self.trail = Some(TrailState {
                        from,
                        to,
                        start: self.tick_target,
                        origin_dy: 0.0,
                        ride: GhostRide::Riding(0.0),
                    });
                }
            }
        }
        self.last_cursor_cell = cursor_cell;
        let ghost = match self.trail {
            Some(st) => {
                let p = ((self.tick_target - st.start) / TRAIL_S) as f32;
                if p >= 1.0 {
                    // Landed: this frame draws `set_trail(None)`, whose
                    // `trail_moved` dirties the ghost's last rows to clear it,
                    // and the tick stops forcing frames.
                    self.trail = None;
                    None
                } else {
                    Some(Trail {
                        from: st.from,
                        to: st.to,
                        progress: p.max(0.0),
                        origin_dy: st.origin_dy,
                        lift: Px::of(0),
                    })
                }
            }
            None => None,
        };
        let look = match self.cursor_trail {
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
                // Looking back forgets where the cursor was, as a resize
                // does: output may have moved it meanwhile, and a trail on
                // the way back would come from a place the reader never saw
                // it leave. A hidden cursor keeps it -- `trail.rs`.
                if looking(&self.term, self.back).index() != 0 {
                    self.chase.snap();
                }
                let style = self.term.cursor_style();
                let rect = followed.map(|f| {
                    let (r, c) = f.cell();
                    self.renderer.cursor_rect(r, c, f.style(style))
                });
                let m = self.renderer.metrics();
                let side = |p: Px| f32::from(u16::try_from(p.value()).unwrap_or(u16::MAX));
                let cell = (side(m.cell_w), side(m.cell_h));
                HostTime::of_secs(self.tick_target)
                    .and_then(|now| self.chase.frame(rect, now, cell))
            }
            crate::config::CursorTrail::Off => None,
        };
        // Handed to the renderer below, once the presenter has settled and
        // the slide's pan is read -- the one read the lift, the scene and the
        // shader's slide all take.
        let trail_look = look;
        // The alternate-screen fade: a veil over the new screen ramping to
        // nothing, advanced and cleared off `tick_target` like the trail.
        let veil = match self.transition {
            Some(st) => {
                let p = ((self.tick_target - st.start) / TRANSITION_S) as f32;
                if p >= 1.0 {
                    // Settled: this frame draws `set_screen_fade(None)`,
                    // which repaints the whole screen once to clear it, and
                    // the tick stops forcing frames.
                    self.transition = None;
                    None
                } else {
                    Some(Transition {
                        progress: p.max(0.0),
                    })
                }
            }
            None => None,
        };
        self.renderer.set_screen_fade(veil);
        // The input manager's composition, at the cursor of the live view:
        // the instance and CPU paths draw it in their convert, the cell path
        // through its overlay below.
        let composition = (!self.composing.is_empty()
            && looking(&self.term, self.back).index() == 0)
            .then(|| {
                let (row, col) = self.term.cursor();
                Composition {
                    row,
                    col,
                    text: self.composing.clone(),
                }
            });
        self.renderer.set_composition(composition.clone());
        // The frame -- the grid, and a legacy scroller's gutter beside it --
        // and the whole surface it is centred in; the letterbox margin
        // between them is the gap.
        let (w, h) = self.renderer.frame_size();
        let (sw, sh) = self.view_px;
        self.pad_px = (
            Px::of(sw.value().saturating_sub(w.value()) / 2),
            Px::of(sh.value().saturating_sub(h.value()) / 2),
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
        // **How often a frame is built over memory the GPU is still
        // reading.** The three writes below -- the glyph map, the resolved
        // styles and the overlay records -- all go into Metal *shared*
        // buffers (`Gpu::styles` lends a `&mut [Resolved]` out of the same
        // allocation it wraps in a `StylesBuffer`), and so does the grid
        // itself, which the cell shader reads in place. Nothing checks
        // whether a frame is in flight until `draw_cells`, which is after
        // all of them, so `Drawn::Busy` reports the hazard rather than
        // preventing it.
        //
        // `pump` does hold the barrier, so this is zero in steady state.
        // What reaches here mid-flight is `relayout` and `zoom`, which call
        // `present` directly -- which is to say: a resize and a font change,
        // the two gestures the owner reported as broken.
        if self.presenter.as_ref().map(Presenter::in_flight) == Some(InFlight::Busy) {
            self.wrote_in_flight.bump();
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
        // The cursor's blink phase at the same moment, kept with the bell's
        // plan when an arm shows the frame.
        let blinks = crate::caret::blinks(
            self.term.cursor_style(),
            self.focus,
            live_cursor(&self.term, self.back),
        );
        let caret = self.caret.plan(seen_at, blinks);
        self.renderer.set_cursor_blink(caret);
        // Image placements for this view, for the two GPU arms: both read the
        // one list `set_images` builds, so an image lands where its line is.
        // The CPU arm's `render_at` places them itself, without the upload
        // snapshots a GPU needs and it does not -- a second converted copy of
        // every visible image, kept for nothing.
        if self.presenter.as_ref().map(Presenter::mode) == Some(Mode::Gpu) {
            self.renderer
                .set_images(&self.term, looking(&self.term, self.back));
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
        // it, so none of them can disagree. A trail rides only where the
        // grid slides -- the cell path, at the live screen, on the primary
        // (`slides`) -- and draws where the screen had it under `stay`.
        let pan = self.slide.pan();
        let drawn = SlideDrawn::of(
            self.presenter.as_ref().map(Presenter::mode),
            self.cell.as_ref(),
        );
        let sliding = slides(
            self.scroll,
            looking(&self.term, self.back),
            self.term.screen(),
            drawn,
        ) && pan.value() > 0;
        let lift = if sliding { pan } else { Px::of(0) };
        self.renderer
            .set_trail(trail_look.map(|l| l.lifted(trail_lift(self.trail_scroll, lift))));
        match (
            self.presenter.as_ref().map(Presenter::mode),
            self.cell.as_mut(),
        ) {
            (Some(Mode::Gpu), Some(cell)) => {
                let t0 = Instant::now();
                // The window `back` lines up the ring: the shader's rotation
                // retreats by that and reads the retained rows where they
                // are, so a scrolled-back frame costs what a live one costs.
                let back = looking(&self.term, self.back);
                let view = self.term.view(back);
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
                // The set's resolved table, brought up to date from the core's
                // log: only the slots written since this set was last built,
                // which on a steady screen is none. Before the map
                // walk, which reads each cell's face style from it.
                self.renderer.sync_styles(
                    &self.term,
                    &mut cell.styles_mem[set][..],
                    &mut cell.style_stamps[set],
                );
                if sliding {
                    // The leaving row is never selected and never holds the
                    // cursor (the shader's row there is past every one).
                    let above = self.term.view(Rows::of_index(back.index() + 1));
                    cell.maps.live.ensure(
                        above,
                        &cell.styles_mem[set][..],
                        &mut self.renderer,
                        &mut cell.candidates,
                    );
                }
                let cursor_at = live_cursor(&self.term, self.back)
                    .map(|(r, c)| (r, c, self.term.cursor_style()));
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
                        let (r, col) = c.cursor(view.cols());
                        Some((r, col, style))
                    }
                    (c, _) => c,
                };
                cell.maps.live.ensure(
                    view,
                    &cell.styles_mem[set][..],
                    &mut self.renderer,
                    &mut cell.candidates,
                );
                let cursor = cursor_at.filter(|&(_, _, style)| self.renderer.cursor_drawn(style));
                // **D4: the window, copied into this frame's slot.** The
                // rows the view names, in screen order, so what the shader
                // reads is a still image of the screen rather than a grid
                // the parser is still writing -- which is what lets `pump`
                // keep feeding while this frame is in flight.
                //
                // `Viewport::row` resolves a row through the rotation and
                // the ring, so the wrap at the ring's end costs nothing
                // here: it is two slices either side of it, and this walks
                // rows rather than one span.
                let stride = self.term.stride().index();
                let (vcols, vrows) = (view.cols().index(), view.rows().index());
                for y in 0..vrows {
                    let src = view.row(Row::from_index(y));
                    let at = y * stride;
                    cell.copies_mem[set][at..at + vcols].copy_from_slice(&src[..vcols]);
                }
                // The leaving row: the line just above the window,
                // `view(back + 1).row(0)`, appended at index `vrows`, where
                // `slide_rows = 1` maps draw row 0 to it with the identity
                // rotation the copy is read under.
                if sliding {
                    let above = self.term.view(Rows::of_index(back.index() + 1));
                    let src = above.row(Row::from_index(0));
                    let at = vrows * stride;
                    cell.copies_mem[set][at..at + vcols].copy_from_slice(&src[..vcols]);
                }
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
                        let at = if p.col.index() > 0 {
                            p.col.index() - 1
                        } else {
                            p.col.index()
                        };
                        let style = view
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
                        .get(c.col.index())
                        .copied()
                        .unwrap_or(PackedCell::EMPTY);
                    cell.exceptions.composed(
                        &mut self.renderer,
                        c,
                        under,
                        &cell.styles_mem[set][..],
                        cursor,
                        view.cols(),
                        if sliding { pan } else { Px::of(0) },
                        &mut cell.overlay_mem[set][..],
                        &mut cell.records,
                    );
                }
                // The trail, thumb and screen fade ride with the records,
                // drawn after the cell passes: the trail under the thumb, and
                // the fade last so it veils everything during a switch.
                cell.records.extend_from_slice(self.renderer.trail());
                cell.records.extend_from_slice(self.renderer.scrollbar());
                cell.records.extend_from_slice(self.renderer.screen_fade());
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
                    ring: Rows::of_index(if sliding { vrows + 1 } else { vrows }),
                    cursor,
                    cursor_color: self.term.cursor_color_in(self.renderer.video()),
                    highlight,
                    overlay: &cell.overlay[set],
                    records: &cell.records,
                    slide: sliding.then_some(pan),
                    set: at,
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
                            if show == Show::Now {
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
                let converted =
                    self.renderer
                        .convert(&self.term, looking(&self.term, self.back), damage);
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
                            if show == Show::Now {
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
                    let back = looking(&self.term, self.back);
                    self.renderer
                        .render_at(&mut self.term, back, &mut target, damage)
                };
                self.stats = stats;
                let clear = self.renderer.clear_color();
                if let Some(p) = &mut self.presenter {
                    if p.present(screen, &self.frame, w, clear) == Presented::Yes {
                        shown = Presented::Yes;
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
            Presented::Yes => self.bell.commit(bell),
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
    fn report(&mut self) {
        if self.last_report.elapsed().as_secs() >= 5 {
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
                    "{} ticks, {} frames; last: {} rows, {} instances, convert {:?}, paint {:?}, cache {:?}",
                    self.ticks.value(),
                    self.frames.value(),
                    self.stats.rows_dirty.value(),
                    self.stats.instances.value(),
                    self.stats.convert,
                    self.stats.composite,
                    self.stats.cache
                ),
            }
            if let Some(line) = self.latency.report() {
                eprintln!("{line}");
            }
            if let Some(line) = self.clips.report() {
                crate::info!("{line}");
            }
            if let Some(line) = self.widths.report() {
                crate::info!("{line}");
            }
            let l = self.links;
            if l.opened.value() + l.revealed.value() + l.refused.value() > 0 {
                crate::info!(
                    "links: {} opened, {} revealed, {} refused",
                    l.opened.value(),
                    l.revealed.value(),
                    l.refused.value()
                );
            }
            self.last_report = std::time::Instant::now();
        }
    }
}

/// What the present path has done, for the five-second report and for
/// `BAUD_FRAME_LOG`.
///
/// `superseded` is the one to read: a frame that finished after the
/// surfaces it drew into had been replaced. Shown, it was the doubled image
/// on a grow and the old font size on a zoom; since `3c7e589` `admits`
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
        // The row height changed, so the slide's cap did too; keep an offset
        // in flight a valid fraction of the new row. The Sixel cursor advance
        // divides by the same height, so re-take it here or every image from
        // now mis-advances -- the stale-geometry trap this repo has hit before.
        self.slide.set_cell_h(self.renderer.metrics().cell_h);
        self.term.set_cell_size(
            cell_px(self.renderer.metrics().cell_w),
            cell_px(self.renderer.metrics().cell_h),
        );
        if let Some(cell) = self.cell.as_mut() {
            cell.maps.rescaled();
        }
        Some(())
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
                "present: {} landed, {} shown, {} superseded, {} deferred, {} given back, {} built over a live frame, last gpu {:?}",
                c.landed,
                c.shown,
                c.superseded,
                c.deferred,
                c.abandoned,
                self.wrote_in_flight.value(),
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
/// This is the axis the crate's example follows, and the one the owner was
/// watching when they said the example changes with the system settings --
/// measured, because the example's body is byte-identical under a light
/// and a dark system while its own band reads "Clear > Light" under both.
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
    glass: &GlassSurface,
    window: &GlassWindow,
    cfg: &crate::config::Glass,
    appearance: Appearance,
) {
    let dark = appearance == Appearance::Dark;
    let strength = if dark { cfg.tint_dark } else { cfg.tint_light };
    let color = (strength > 0.0).then(|| hsb(cfg.hue, cfg.saturation, 0.10, strength));
    glass.set_tint_color(color.as_deref());
    window.set_has_shadow(match cfg.shadow {
        crate::config::Shadow::Always => true,
        crate::config::Shadow::Never => false,
        crate::config::Shadow::Auto => dark,
    });
}

/// The palette an appearance resolves to, at the configured opacity: the
/// built-in half, with the theme file named for it laid over it.
fn theme_for(
    appearance: Appearance,
    glass: &crate::config::Glass,
    floor: baud_render::Floor,
    halves: &crate::palette::Halves,
) -> Theme {
    let half = match appearance {
        Appearance::Dark => crate::palette::Half::Dark,
        Appearance::Light => crate::palette::Half::Light,
    };
    crate::palette::half_theme(half, glass, floor, halves)
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
        floor: baud_render::Floor,
        halves: &crate::palette::Halves,
    ) -> Themes {
        Themes {
            dark: theme_for(Appearance::Dark, glass, floor, halves),
            light: theme_for(Appearance::Light, glass, floor, halves),
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
    let t = Themes::of(glass, floor, &crate::palette::Halves::default());
    (t.dark.floor, t.light.floor)
}

/// The two themes `Themes::of` builds from `halves`, for the palette test.
#[cfg(test)]
pub(crate) fn themes_with(halves: &crate::palette::Halves) -> (Theme, Theme) {
    let t = Themes::of(
        &crate::config::Glass::defaults(),
        baud_render::Floor::DEFAULT,
        halves,
    );
    (t.dark, t.light)
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
/// theme never switches. Jev, logged: this rule over dropping on every
/// callback, 0.81.
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
    /// The kind of the key being handled, from `key()` to the `typed()` it
    /// reaches. A printable key goes through the input manager, whose
    /// `text()` and `command()` carry no repeat flag, so `key()` notes it
    /// here first and `typed()` takes it, so a repeat marks one event. A
    /// repeating key that types nothing leaves it set until the next `key()`
    /// resets it, and only a `text()` from outside a key press -- an input
    /// method committing on a click -- could take it in between.
    key_kind: KeyEventKind,
    /// A Cmd press over a link, until its release: the gesture is the
    /// terminal's, and `crate::link::gesture` says what each event means.
    armed: Option<crate::link::Armed>,
    /// A drag of the scrollbar's thumb: where it was grabbed, in frame
    /// pixels, and where its top was.
    bar_drag: Option<BarDrag>,
    /// Whether the press in progress is the scrollbar's: its drags and its
    /// release are then the scrollbar's too, so neither the selection nor a
    /// tracking program sees a release without its press.
    grab: Grab,
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
    /// The appearance the palette was last set for under `auto`: `None`
    /// before the first, and always under a pinned theme.
    palette_for: Option<Appearance>,
    /// The window's material, and the window itself, so a light/dark flip
    /// can re-tint the one and re-shadow the other. Both are retains: a
    /// clone of either addresses the one AppKit object.
    ///
    /// The window one closes a cycle -- the window owns the view, the
    /// view owns this driver, and this driver retains the window -- which
    /// is a leak of exactly one window on a process that never tears its
    /// window down before it exits, and is the same trade the style
    /// observer is deliberately `forget`ten for.
    glass: GlassSurface,
    window: GlassWindow,
    /// The title the window was last given, so a program's `OSC 0` or
    /// `OSC 2` reaches it once rather than on every read.
    titles: crate::title::Shown,
    /// The titlebar mark, repositioned on every resize. A manual autoresizing
    /// mask lost it after a fast drag to a tiny window -- the frame clamped
    /// and never came back -- so the driver places it itself instead: `icon_x`
    /// from the left, centred in the `icon_inset` strip at the top of its
    /// superview, whose height it reads each time.
    icon: Option<Retained<NSImageView>>,
    /// The icon's superview (the content container), whose height gives the
    /// window top; read rather than `superview()`, which is `unsafe`.
    icon_super: Retained<NSView>,
    icon_x: CGFloat,
    icon_size: CGFloat,
    icon_inset: CGFloat,
}

use baud_core::Moved as Ring;

/// Where the scrollback window sits after a resize moved the ring under
/// it, given how the rotation moved and how much history is retained.
///
/// **A window on the live screen stays on the live screen.** This is the
/// owner's rule -- *"we should not change the scroll ever"* -- and the
/// bug it fixes was visible as duplicated content: zooming all the way in
/// shrinks the grid, a shorter screen retires the rows the cursor needs,
/// and the old form added that retirement to `back` whatever `back` was.
/// From zero that scrolls a user who was at their prompt up into the
/// history, so the screen fills with the prompt blocks the shell had
/// already written and the live one is somewhere below the bottom edge.
/// Zoom out and in again and it compounds. The recording shows a banner
/// four times that the shell printed once.
///
/// `pump` has always had this guard for output scrolling and `relayout`
/// did not, which is the whole difference: one place moved the window and
/// the other did not, for the same event.
///
/// What it gives up is stated rather than hidden: while a *shrink* is in
/// progress the first visible line now scrolls away, because the rows the
/// cursor needs come off the top. That is what every terminal does, and
/// it is what the old code was avoiding at the cost above.
fn anchored(back: Rows, moved: Ring, retained: Rows) -> Rows {
    if back.index() == 0 {
        return back;
    }
    let at = match moved {
        // Rows left the top, so the window is that much further from the
        // live screen if it is to keep showing the lines it was showing.
        Ring::Retired(n) => back.index().saturating_add(n.index()),
        Ring::Restored(n) => back.index().saturating_sub(n.index()),
    };
    Rows::of_count(at.min(retained.index()))
}

/// Where both rings stood before a read or a resize: the live surface's,
/// for the selection's line count and a slide, and the primary's, for a
/// window over the history.
#[derive(Clone, Copy)]
struct Before {
    live: RowRot,
    history: baud_core::HistoryMark,
    epoch: baud_core::ScreenEpoch,
    region: baud_core::RegionScrolls,
    erased: baud_core::HistoryErasures,
    resets: baud_core::Resets,
    rows: baud_core::RowMoves,
    /// The scrolling region, top and bottom, for a read that sets another
    /// after copying in this one.
    band: (Row, Row),
}

impl Before {
    fn of(term: &Term<'_>) -> Before {
        Before {
            live: term.rotation(),
            history: term.history_mark(),
            epoch: term.screen_epoch(),
            region: term.region_scrolls(),
            erased: term.history_erasures(),
            resets: term.resets(),
            rows: term.row_moves(),
            band: term.scroll_region(),
        }
    }

    /// The readings a pump takes before it feeds, with the core's watch on
    /// the history's erasures armed at them, so the first erasure the feed
    /// makes is kept for [`restart`] whatever erases after it.
    fn watched(term: &mut Term<'_>) -> Before {
        term.watch_erasures();
        Before::of(term)
    }
}

/// Whether a read erased the primary's history -- `CSI 3 J` or `RIS`, from
/// either screen -- by the core's count, because nothing else across a read
/// says so: the erase scrolls nothing, and lines after it in the same read
/// refill the history to a count that hides it.
fn erased(term: &Term<'_>, before: Before) -> bool {
    term.history_erasures() != before.erased
}

/// Whether a read's region scrolls left `sel` naming rows that did not move.
/// A selection is numbered by the lines the primary retired, which is right
/// for the region's rows and the history -- they moved -- and wrong for the
/// rows under a region, which stayed: renumbered, a selection there would
/// land on the rows above it. foot cancels a selection touching rows a
/// partial scroll left (`term_scroll_partial`); this does the same, and only
/// for those rows, so a selection in the history survives every line a
/// progress bar's region prints. The rows are the ones under the region in
/// force at the read's end, numbered by `scrolled` from before it; a read that
/// reset its region leaves none to read, and then every live row counts.
fn stale_under_region(
    sel: &Selection,
    term: &Term<'_>,
    before: Before,
    scrolled: Scrolled,
) -> bool {
    if whole_screen_scrolled(term, before) {
        return false;
    }
    let (top, bot) = term.scroll_region();
    let last = Row::from_index(term.geometry().1.index().saturating_sub(1));
    let first = match top == Row::ZERO && bot < last {
        true => Row::from_index(bot.index() + 1),
        false => Row::ZERO,
    };
    sel.touches(
        scrolled.line(Rows::ZERO, first),
        scrolled.line(Rows::ZERO, last),
    )
}

/// Whether a read moved, in place, rows of the live screen that `sel` names:
/// an `IL` or `DL`, or a region scroll that copies its rows -- a region in
/// the middle of the screen, a short top region on the alternate screen,
/// `SD` and `RI` in any partial region, and every vertical scroll under side
/// margins, the whole screen included, which moves only the rectangle. Rows
/// are tested and columns are not, so a selection beside the rectangle on
/// its rows is stale too, though its text did not move. No ring moves,
/// so the selection's lines still name the same screen rows while the text
/// on them has moved, and it would copy what took their place.
fn stale_in_moved_rows(
    sel: &Selection,
    term: &Term<'_>,
    before: Before,
    scrolled: Scrolled,
    moved: Ring,
) -> bool {
    if term.row_moves() == before.rows {
        return false;
    }
    // The rows a copy can move: the region in force at the read's end, which
    // is every live row once a read reset it -- `IL` and `DL` act from the
    // cursor to its bottom, inside it. Lines from the region's top as the
    // read found them to its bottom as the read left them, since a read can
    // retire lines through the whole screen around the copy.
    //
    // **Every live row when the read's two regions differ.** A read can copy
    // in one region and then set another -- stacked panes, each drawn under
    // its own -- and the region it ends in never moved. A region set and set
    // back within one read still reads as one; only a count of the rows each
    // copy moved would see it.
    let (top, bot) = match term.scroll_region() {
        r if r == before.band => r,
        _ => (
            Row::ZERO,
            Row::from_index(term.geometry().1.index().saturating_sub(1)),
        ),
    };
    let after = match moved {
        Ring::Retired(n) => scrolled.advanced(n),
        Ring::Restored(_) => scrolled,
    };
    sel.touches(scrolled.line(Rows::ZERO, top), after.line(Rows::ZERO, bot))
}

/// Whether every line a read retired left with the whole screen: no scroll
/// of a region smaller than the screen sent one to the history. A region
/// anchored at the top retires its line the way the whole screen does, and
/// the rows under it stay where they were -- so a slide that pans the whole
/// grid would carry a status line or a dashboard up with the text above it.
/// By the core's count rather than by the region at the read's end, because
/// a program can set a region, scroll it and reset it inside one read.
fn whole_screen_scrolled(term: &Term<'_>, before: Before) -> bool {
    term.region_scrolls() == before.region
}

/// Whether a read switched surfaces -- by the core's epoch, not by the
/// screen at its two ends: a read that left the alternate screen and
/// entered it again ends where it began, on a surface `?1049` cleared or
/// `?47` showed as it was left some time before, and a selection or a slide
/// from before names rows it cannot vouch for.
fn switched(term: &Term<'_>, before: Before) -> bool {
    term.screen_epoch() != before.epoch
}

/// The oldest line a selection may name: as far behind the live screen's
/// top as the live surface keeps -- the primary's history, and nothing on
/// the alternate screen, where the history count is still the primary's.
fn oldest(term: &Term<'_>, scrolled: Scrolled) -> AbsLine {
    let retained = Rows::of_count(term.scrollback().len());
    scrolled.oldest(term.looking_back(retained))
}

/// How the live surface moved over a read, for the selection's line count
/// and a slide: **nothing across a switch**, whose two rotations are two
/// rings' -- the primary's, round thousands of rows, and the alternate
/// screen's, round one screen, or the alternate screen's before and after
/// an entry reset it -- so their difference is no movement of anything,
/// and read as one it subtracted past zero. A switch drops the selection
/// and settles a slide, whichever way the read went.
/// What the live screen did across a read, for the slide, the ride and the
/// selection's numbering: `live_moved`'s answer, or nothing when a program's
/// page width resized the grid inside the read (`honour`). The reflow's
/// re-cut retires rows that no output scrolled, and `relayout`, which a
/// drag reaches outside the pump, starts no slide for one either; the
/// selection is gone with the resize, so nothing it numbers is left.
fn read_moved(term: &Term<'_>, before: Before, resized: Option<crate::width::Grid>) -> Ring {
    match resized {
        Some(_) => Ring::Retired(Rows::ZERO),
        None => live_moved(term, before),
    }
}

fn live_moved(term: &Term<'_>, before: Before) -> Ring {
    if switched(term, before) {
        return Ring::Retired(Rows::ZERO);
    }
    term.moved_since(before.live)
}

/// The window the user looks through, for everything that reads one: the
/// core's clamp of `back`, the one `Term::view` applies. On the alternate
/// screen, which keeps no history, it is the live screen whatever `back`
/// says -- so the thumb, the cursor, a selection and the link under a click
/// agree with what is drawn, rather than with a count that is the primary's.
fn looking(term: &Term<'_>, back: Rows) -> Rows {
    term.looking_back(back)
}

/// **`?1001`'s region, through the selection's highlight**, on the view
/// `back` lines up: `[[source: ctlseqs]]`'s highlight tracking "highlights
/// the region covered by the mouse within that range until button
/// release", and the core says what that region is (`Term::highlight`).
/// Its cells are the live screen's, numbered as a selection's are, and the
/// region is a stream from the earlier of its two ends to the later -- the
/// core keeps them in the order the program and the mouse gave them. `None`
/// when nothing is tracked or none of it is on this view. Drawn in place of
/// the selection while it lasts and never stored as one, so Cmd-C still
/// copies the user's selection and the user's selection is back after the
/// release.
fn tracked_highlight(term: &Term<'_>, scrolled: Scrolled, back: Rows) -> Option<Highlight> {
    let h = term.highlight()?;
    let point = |c: baud_core::CellAt| Point::new(scrolled.line(Rows::ZERO, c.row), c.col);
    let (a, b) = (point(h.start), point(h.end));
    let span = Span {
        start: a.min(b),
        end: a.max(b),
    };
    let view = term.view(back);
    Selection::at(span, Grain::Cell).highlight(scrolled.top(back), view.rows(), view.cols())
}

/// `?1001`'s region on screen: the core's handshake drawn through the
/// selection's highlight (`tracked_highlight`).
#[cfg(test)]
mod highlight_tests {
    use super::tracked_highlight;
    use baud_core::{CellAt, MouseButton, MouseEvent, MouseKind, MouseMods, Owned, Term};
    use baud_domain::{BytePos, Col, ReadLen, Row, Rows, WinSize};
    use baud_render::{Highlight, Region, Scrolled};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 10,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// A cell by column and row, zero-based.
    const fn at(col: Col, row: Row) -> CellAt {
        CellAt { col, row }
    }

    fn left(t: &mut Term<'_>, kind: MouseKind, at: CellAt) {
        t.mouse_report(MouseEvent {
            button: MouseButton::Left,
            mods: MouseMods::NONE,
            kind,
            col: at.col,
            row: at.row,
            px: None,
        });
        t.clear_reply();
    }

    /// The stream highlight from one cell to another, as the view draws it.
    fn drawn(from: CellAt, to: CellAt) -> Option<Highlight> {
        Some(Highlight {
            from: (from.row, from.col),
            to: (to.row, to.col),
            region: Region::Stream,
        })
    }

    /// **From the answer to the release, the region is what is drawn**, a
    /// stream from the earlier end to the later, whichever way the mouse
    /// went; nothing before the answer and nothing after the release.
    #[test]
    fn the_program_s_region_is_drawn_through_the_selection_highlight() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let live = |t: &Term<'_>| tracked_highlight(t, Scrolled::START, Rows::ZERO);
        feed(&mut t, b"\x1b[?1001h");
        left(
            &mut t,
            MouseKind::Press,
            at(Col::at_index(3), Row::from_index(2)),
        );
        assert_eq!(live(&t), None, "nothing while the answer is owed");
        feed(&mut t, b"\x1b[1;4;3;1;10T");
        left(
            &mut t,
            MouseKind::Motion,
            at(Col::at_index(7), Row::from_index(5)),
        );
        assert_eq!(
            live(&t),
            drawn(
                at(Col::at_index(3), Row::from_index(2)),
                at(Col::at_index(7), Row::from_index(5))
            )
        );
        left(
            &mut t,
            MouseKind::Motion,
            at(Col::at_index(1), Row::from_index(1)),
        );
        assert_eq!(
            live(&t),
            drawn(
                at(Col::at_index(1), Row::from_index(1)),
                at(Col::at_index(3), Row::from_index(2))
            ),
            "sorted for drawing"
        );
        left(
            &mut t,
            MouseKind::Release,
            at(Col::at_index(1), Row::from_index(1)),
        );
        assert_eq!(live(&t), None, "gone at the release");
    }

    /// **On a view scrolled back it sits where the live rows are drawn**,
    /// and what falls below the view is clipped to it.
    #[test]
    fn on_a_view_scrolled_back_it_moves_down_with_the_live_rows() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let lines: Vec<u8> = (0..40)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        feed(&mut t, &lines);
        assert_eq!(t.scrollback().len(), 31);
        // The count the driver keeps: the live top is line 31.
        let scrolled = Scrolled::START.advanced(Rows::of_count(31));
        feed(&mut t, b"\x1b[?1001h");
        left(
            &mut t,
            MouseKind::Press,
            at(Col::at_index(3), Row::from_index(2)),
        );
        feed(&mut t, b"\x1b[1;4;3;1;10T");
        left(
            &mut t,
            MouseKind::Motion,
            at(Col::at_index(7), Row::from_index(4)),
        );
        assert_eq!(
            tracked_highlight(&t, scrolled, Rows::of_count(3)),
            drawn(
                at(Col::at_index(3), Row::from_index(5)),
                at(Col::at_index(7), Row::from_index(7))
            ),
            "three lines back, three rows down"
        );
        left(
            &mut t,
            MouseKind::Motion,
            at(Col::at_index(7), Row::from_index(8)),
        );
        assert_eq!(
            tracked_highlight(&t, scrolled, Rows::of_count(3)),
            drawn(
                at(Col::at_index(3), Row::from_index(5)),
                at(Col::at_index(19), Row::from_index(9))
            ),
            "the end below the view is clipped to its last cell"
        );
    }
}

/// Where the window sits in the ring, for the scrollback's thumb.
fn window_of(term: &Term<'_>, back: Rows) -> Window {
    Window {
        // What the live surface can reach, by the same clamp: nothing on the
        // alternate screen, so no thumb is drawn there even at the bottom
        // of its travel, where a reveal would otherwise show one over the
        // primary's history.
        retained: term.looking_back(Rows::of_count(term.scrollback().len())),
        back: looking(term, back),
        rows: term.viewport().rows(),
    }
}

/// The cursor to draw: the live screen's, while the window is on it.
fn live_cursor(term: &Term<'_>, back: Rows) -> Option<(Row, Col)> {
    (term.cursor_visible() && looking(term, back).index() == 0).then(|| term.cursor())
}

/// The cursor a trail follows, while the window is on the live screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TrailCursor {
    /// The cursor as it is drawn.
    Shown(Row, Col),
    /// Where a program put the cursor before it hid it
    /// (`Term::placed_cursor`): helix's selection, where helix draws its own
    /// block. Never drawn here -- the program hid it.
    Placed(Row, Col),
}

impl TrailCursor {
    fn cell(self) -> (Row, Col) {
        match self {
            TrailCursor::Shown(r, c) | TrailCursor::Placed(r, c) => (r, c),
        }
    }

    /// The shape the trail arrives at: the drawn cursor's own, and the whole
    /// cell for a placement, because the program draws a block there whatever
    /// shape the terminal's cursor is configured to -- the smear's hole has to
    /// be that block, or a bar-shaped hole leaves the trail over it.
    fn style(self, drawn: baud_core::CursorStyle) -> baud_core::CursorStyle {
        match self {
            TrailCursor::Shown(..) => drawn,
            TrailCursor::Placed(..) => baud_core::CursorStyle::SteadyBlock,
        }
    }
}

/// **The cursor a trail follows**: the one drawn, and while it is hidden,
/// where a program placed it before hiding it. Helix hides the terminal's
/// cursor for good and draws its own at the selection, moving the hidden one
/// there first on every frame, so a trail that followed only the drawn cursor
/// drew nothing on `gg` or `ge`. A hidden cursor no program placed is not
/// followed: the trail keeps where it last was (`trail.rs`).
fn trail_cursor(term: &Term<'_>, back: Rows) -> Option<TrailCursor> {
    if looking(term, back).index() != 0 {
        return None;
    }
    match term.cursor_visible() {
        true => {
            let (r, c) = term.cursor();
            Some(TrailCursor::Shown(r, c))
        }
        false => term.placed_cursor().map(|(r, c)| TrailCursor::Placed(r, c)),
    }
}

/// A window over the history after a read or a resize moved the rings
/// under it: anchored by the **primary's** movement, which holds the history
/// whatever screen is up. The live rotation is the alternate screen's while
/// it is up, so its scrolling pushed the window up a history that had not
/// moved, and a read that switched screens compared two rings.
///
/// **A read that erased the history puts it on the live screen**: the lines
/// it showed are gone, and anchoring by the movement -- none, for an erase
/// -- clamped it to whatever refilled the history in the same read, so the
/// same bytes landed live or on the oldest new line by where the read ended.
fn after_read(term: &Term<'_>, back: Rows, before: Before) -> Rows {
    if erased(term, before) {
        return Rows::ZERO;
    }
    let retained = Rows::of_count(term.scrollback().len());
    anchored(back, term.history_moved_since(before.history), retained)
}

/// Resize the terminal and say where a window over the history is after it.
/// The resize and the anchor are one step, because a change of width
/// re-cuts the lines under the window: anchoring it afterwards by how far the
/// window's top line moved counted the growth of every line above it,
/// including the lines above the one the window shows, and put it on another
/// line. The core maps the window's first row through the re-cut
/// (`Term::resize_holding`), and a live window stays live.
fn resized_window(term: &mut Term<'_>, ws: &WinSize, back: Rows) -> Rows {
    term.resize_holding(ws, back).1
}

/// The window a key or a paste leaves: the live screen, where the key's
/// effect is -- on the primary. On the alternate screen the live screen is
/// already what is drawn, and the primary's window is kept for when the
/// program leaves, as foot keeps a view per grid and resets only the live
/// one on a key (`docs/research/notes/l-scrollbar.md`).
fn snapped(term: &Term<'_>, back: Rows) -> Rows {
    match term.screen() {
        Screen::Normal => Rows::ZERO,
        Screen::Alternate => back,
    }
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

/// Whether a read reset the terminal -- `RIS`, from either screen -- by the
/// core's count, because nothing else across a read says so: a reset scrolls
/// nothing and keeps the rotation, so on a terminal with no history both ends
/// of the read look alike, and [`erased`] moves on `CSI 3 J` too, which keeps
/// the screen and a selection on it (note V, V1).
fn reset_in(term: &Term<'_>, before: Before) -> bool {
    term.resets() != before.resets
}

/// **What a read that put another surface up, or reset the terminal, owes
/// `kept`**, and what the pump does after: `before` was taken with the
/// erasure watch armed ([`Before::watched`]), and `back` is the primary's
/// window as the pump found it.
///
/// Note V's V1, the owner's answer of 2026-09-25: when the reset count moves,
/// the selection goes -- it names cells the power-on state does not have, as
/// foot's hard reset and xterm's `RIS` both drop theirs -- the guesses drawn
/// ahead of the shell are dropped as contradicted, the wheel's part-line
/// settles, and a window over the history that came to the live screen
/// reveals its thumb, as a key's snap does (V1's first fork). **The window is
/// the one the reset took live, at the reset's byte**, so the answer is the
/// same however the bytes fell into pumps while nothing but an erasure moves
/// the window inside one (a full-screen `SD`, `RI` at the top or a prompt
/// redraw in the same pump still depends on the batching, until the core
/// keeps the window, V2): the primary's window is drawn
/// only while the primary is up, and any erasure takes it live, so only the
/// first erasure in a pump can reveal, and only a reset made from the
/// primary while the window was off the live screen. Read at the pump's
/// edges it was the window drawn before the pump, and `?1049h` then `RIS`
/// revealed in one pump and not in two. The window and
/// the slide are [`after_read`]'s and the pump's erase arm, which `RIS` moves
/// too. **No fade on a primary reset** (V1's second fork): it is not a switch,
/// so [`Surface`] stays the epoch's; a reset from the alternate screen is
/// both, and fades as a switch does.
fn restart(term: &Term<'_>, before: Before, back: Rows, kept: Kept<'_>) -> (Surface, Thumb) {
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
    let thumb = match (term.first_erasure(before.erased), back.index()) {
        (Some(Erasure::Reset(Screen::Normal)), 1..) => Thumb::Reveal,
        _ => Thumb::Leave,
    };
    (surface, thumb)
}

/// **A window over the history is the primary screen's.** The alternate
/// screen keeps none, so a window left scrolled back when a program enters
/// it is the live screen for everything that reads one, and the primary's
/// ring alone moves it. `docs/research/notes/l-scrollbar.md` is the spec,
/// after foot, which keeps a view per grid.
#[cfg(test)]
mod alt_window_tests {
    use super::{
        Before, Hold, Ring, after_read, erased, hold_after, live_cursor, live_moved, looking,
        oldest, resized_window, snapped, stale_in_moved_rows, stale_under_region, switched,
        whole_screen_scrolled, window_of,
    };
    use baud_core::Zone;
    use baud_core::{Owned, Term};
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

    fn drawn(t: &Term<'_>, back: Rows) -> bool {
        let widths = crate::reveal::widths(crate::reveal::Scale(2.0));
        let size = (Px::of(800), Px::of(500));
        thumb(window_of(t, back), size, Px::of(10), widths, Hover::Rest).is_some()
    }

    #[test]
    fn on_the_alternate_screen_the_window_is_the_live_screen() {
        with_history(|t| {
            let back = Rows::of_count(3);
            // The control, on the primary: three lines up, a thumb, and no
            // cursor -- it belongs to the live screen below the window.
            assert_eq!(looking(t, back).index(), 3);
            assert!(drawn(t, back), "a thumb while looking back");
            assert_eq!(live_cursor(t, back), None);
            feed(t, b"\x1b[?1049h\x1b[Hvi");
            assert_eq!(looking(t, back).index(), 0, "nothing behind the screen");
            assert!(!drawn(t, back), "no thumb over a screen with no history");
            assert!(
                live_cursor(t, back).is_some(),
                "the program that owns the screen owns the cursor"
            );
        });
    }

    #[test]
    fn the_alternate_screens_output_does_not_move_the_primarys_window() {
        with_history(|t| {
            let mut back = Rows::of_count(3);
            let read = |t: &mut Term<'_>, bytes: &[u8], back: &mut Rows| {
                let before = Before::of(t);
                feed(t, bytes);
                *back = after_read(t, *back, before);
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
            let before = Before::of(t);
            feed(t, b"a\r\nb\r\n\x1b[?1049h\x1b[10;1Hw\r\nx\r\ny\r\nz\r\n");
            assert_eq!(after_read(t, Rows::of_count(3), before).index(), 5);
        });
        // Out of it within one read: four on the alternate, one on the
        // primary after it.
        with_history(|t| {
            feed(t, b"\x1b[?1049h\x1b[10;1H");
            let before = Before::of(t);
            feed(t, b"w\r\nx\r\ny\r\nz\r\n\x1b[?1049lc\r\n");
            assert_eq!(after_read(t, Rows::of_count(3), before).index(), 4);
        });
        // In and out within one read: the one case the live rotation
        // already answered, because both its ends are the primary's -- kept
        // so the history mark is held to it too.
        with_history(|t| {
            let before = Before::of(t);
            feed(
                t,
                b"a\r\nb\r\n\x1b[?1049h\x1b[10;1Hw\r\nx\r\n\x1b[?1049lc\r\n",
            );
            assert_eq!(after_read(t, Rows::of_count(3), before).index(), 6);
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

    /// **A region retire drops a selection on the rows under the region, and
    /// only there.** The rows under it stayed, so the count that numbers a
    /// selection is wrong for them alone; a selection in the region moved with
    /// its rows. A whole-screen scroll moves every row, and a read that reset
    /// its region leaves none to read, so every live row counts then.
    #[test]
    fn a_region_retire_drops_a_selection_only_on_the_rows_under_it() {
        use baud_domain::{Col, Row};
        use baud_render::{Grain, Selection, Span};
        with_history(|t| {
            feed(t, b"\x1b[1;5r\x1b[7;1Hdash\x1b[3;1Hpane");
            let scrolled = Scrolled::START;
            let sel = |t: &Term<'_>, y: usize| {
                let r = Row::from_index(y);
                Selection::at(
                    Span::cell(
                        t.row(r).expect("a row on the grid"),
                        scrolled.line(Rows::ZERO, r),
                        Col::ZERO,
                    ),
                    Grain::Cell,
                )
            };
            let (under, pane) = (sel(t, 6), sel(t, 2));
            let before = Before::of(t);
            feed(t, b"\x1b[5;1Hx\r\ny\r\n");
            assert!(
                !whole_screen_scrolled(t, before),
                "the control: a region retired"
            );
            assert!(stale_under_region(&under, t, before, scrolled), "under it");
            assert!(!stale_under_region(&pane, t, before, scrolled), "in it");
            feed(t, b"\x1b[r");
            let before = Before::of(t);
            feed(t, b"\x1b[10;1Hz\r\n");
            assert!(whole_screen_scrolled(t, before), "the control: the screen");
            assert!(
                !stale_under_region(&under, t, before, scrolled),
                "every row moved"
            );
            let before = Before::of(t);
            feed(t, b"\x1b[1;5r\x1b[5;1Hw\r\n\x1b[r");
            assert!(
                stale_under_region(&pane, t, before, scrolled),
                "no region left"
            );
        });
    }

    /// **And the same on the alternate screen**, where vim's tall region
    /// rotates the ring and the status line under it stays. The core counted
    /// region scrolls on the primary only, so here the read looked like a
    /// whole-screen scroll and a selection on the status line was renumbered
    /// onto the row above it.
    #[test]
    fn a_region_rotation_on_the_alternate_screen_drops_a_selection_under_it() {
        use baud_domain::{Col, Row};
        use baud_render::{Grain, Selection, Span};
        with_history(|t| {
            // A tall region on this ten-row screen, `1;8`, so it rotates on
            // the alternate screen as vim's `1;45` does on fifty.
            feed(t, b"\x1b[?1049h\x1b[1;8r\x1b[9;1Hstat\x1b[3;1Hpane");
            let scrolled = Scrolled::START;
            let sel = |t: &Term<'_>, y: usize| {
                let r = Row::from_index(y);
                Selection::at(
                    Span::cell(
                        t.row(r).expect("a row on the grid"),
                        scrolled.line(Rows::ZERO, r),
                        Col::ZERO,
                    ),
                    Grain::Cell,
                )
            };
            let (under, pane) = (sel(t, 8), sel(t, 2));
            let before = Before::of(t);
            let rot = t.rotation();
            feed(t, b"\x1b[8;1Hx\r\ny\r\n");
            assert!(
                t.rotation() != rot,
                "the control: the region rotated the alternate ring"
            );
            assert!(
                !whole_screen_scrolled(t, before),
                "and that was a region, not the whole screen"
            );
            assert!(stale_under_region(&under, t, before, scrolled), "under it");
            assert!(!stale_under_region(&pane, t, before, scrolled), "in it");
        });
    }

    /// **Rows a region moved in place drop a selection on them, and only
    /// there.** A region in the middle of the screen scrolls by copying its
    /// rows, so no ring moves and nothing renumbers the selection: one on the
    /// region's rows named the same screen rows while their text went up a
    /// row. Above the region and under it nothing moved, and the history is
    /// not the live screen. A read that set a region, scrolled it and reset
    /// it leaves none to read, so every live row counts; `IL` with no region
    /// moves the rows from the cursor down.
    #[test]
    fn rows_moved_in_place_drop_a_selection_on_them_and_only_there() {
        use baud_domain::{Col, Row};
        use baud_render::{Grain, Selection, Span};
        with_history(|t| {
            feed(t, b"\x1b[3;1Habove\x1b[5;1Hpane\x1b[8;1Hunder\x1b[4;6r");
            // Numbered as the driver would have it after the thirty-one
            // lines the history holds, so a line behind the screen has a
            // number of its own rather than the floor's.
            let scrolled = Scrolled::START.advanced(Rows::of_count(31));
            let at = |t: &Term<'_>, back: usize, y: usize| {
                let r = Row::from_index(y);
                Selection::at(
                    Span::cell(
                        t.row(r).expect("a row on the grid"),
                        scrolled.line(Rows::of_count(back), r),
                        Col::ZERO,
                    ),
                    Grain::Cell,
                )
            };
            let (above, pane, under) = (at(t, 0, 2), at(t, 0, 4), at(t, 0, 7));
            let history = at(t, 5, 0);
            let stale = |t: &Term<'_>, before: Before, sel: &Selection| {
                stale_in_moved_rows(sel, t, before, scrolled, live_moved(t, before))
            };
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
            assert!(stale(t, before, &pane), "in the region");
            assert!(!stale(t, before, &above), "above it");
            assert!(!stale(t, before, &under), "under it");
            assert!(!stale(t, before, &history), "in the history");
            let before = Before::of(t);
            feed(t, b"x");
            assert!(!stale(t, before, &pane), "a read that moved no row");
            let before = Before::of(t);
            feed(t, b"\x1b[6;1H\n\x1b[r");
            assert!(stale(t, before, &above), "no region left: every live row");
            assert!(!stale(t, before, &history), "and still not the history");
            let before = Before::of(t);
            feed(t, b"\x1b[9;1H\x1b[L");
            assert!(stale(t, before, &under), "IL");
            // Lines retired through the whole screen before the copy, in the
            // same read: row 8 was under the region when the read began, and
            // two lines later it was the region's last row when the copy
            // moved it. The read begins and ends in the one region, so it is
            // the numbering and not the change of region that reaches it.
            feed(t, b"\x1b[4;6r");
            let before = Before::of(t);
            feed(t, b"\x1b[r\x1b[10;1H\n\n\x1b[4;6r\x1b[6;1H\n");
            assert_eq!(live_moved(t, before), Ring::Retired(Rows::of_count(2)));
            assert!(
                stale(t, before, &under),
                "a row the retired lines carried into the region"
            );
        });
    }

    /// **A region set after the copy does not hide it.** A read that copies
    /// in one region and then sets another -- stacked panes, each drawn
    /// under its own -- ends with a region that never moved, so the rows of
    /// the region in force at the read's end missed the pane's line that
    /// went up a row under the selection. The read's two regions differ, so
    /// the rows it can have moved are every live row; the history is still
    /// not the live screen.
    #[test]
    fn a_region_set_after_the_copy_still_drops_a_selection_it_moved() {
        use baud_domain::{Col, Row};
        use baud_render::{Grain, Selection, Span};
        with_history(|t| {
            feed(t, b"\x1b[5;1Hpane");
            let scrolled = Scrolled::START.advanced(Rows::of_count(31));
            let at = |t: &Term<'_>, back: usize, y: usize| {
                let r = Row::from_index(y);
                Selection::at(
                    Span::cell(
                        t.row(r).expect("a row on the grid"),
                        scrolled.line(Rows::of_count(back), r),
                        Col::ZERO,
                    ),
                    Grain::Cell,
                )
            };
            let (pane, history) = (at(t, 0, 4), at(t, 5, 0));
            let before = Before::of(t);
            feed(t, b"\x1b[4;6r\x1b[6;1H\n\x1b[8;9r");
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
            let stale = |sel: &Selection| {
                stale_in_moved_rows(sel, t, before, scrolled, live_moved(t, before))
            };
            assert!(stale(&pane), "on the pane's moved line");
            assert!(!stale(&history), "not in the history");
        });
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
            let before = Before::of(t);
            let _ = t.resize(&WinSize {
                ws_col: 20,
                ws_row: 8,
            });
            assert_eq!(after_read(t, Rows::of_count(3), before).index(), 5);
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
                let back = resized_window(&mut t, &ws(to), back);
                assert_eq!(
                    text(&t, back),
                    viewed,
                    "{arm}, {from} to {to} columns: the window moved to another line"
                );
                // A live window stays live, whatever the re-cut did.
                assert_eq!(resized_window(&mut t, &ws(from), Rows::ZERO), Rows::ZERO);
            }
        }
    }

    /// **A full-screen program started from the prompt is not a prompt
    /// opening.** A contradicted prediction holds prediction until the
    /// shell's next prompt, and the hold is released when the zone turns to
    /// `Prompt`. Each screen has its own zone, so a program a line-editor
    /// widget opens with no `C` -- fzf full-screen, `edit-command-line` --
    /// turned the live zone to `Output` and back, which read as a prompt
    /// opening when it closed. The control is a real next prompt, which
    /// releases the hold.
    #[test]
    fn an_alternate_screen_round_trip_is_not_a_prompt_opening() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b]133;P;k=i\x07$ ");
        let (mut hold, mut last) = hold_after(&t, Hold::Open, Zone::Output);
        assert!(hold == Hold::Open, "the control: a prompt opened");
        // A prediction the echo contradicted.
        hold = Hold::UntilNextPrompt;
        for read in [&b"\x1b[?1049h\x1b[Hpicker"[..], b"\x1b[?1049l"] {
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
        const FEED: &str = concat!("crate::reads::", "feed(");
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
    /// pump and the resize. This holds the source to them instead: every
    /// line that names the raw window hands it to one of the helpers, or is
    /// one of the lines below, each a write or a path the alternate screen
    /// cannot reach -- the exact list, so a new raw reader fails here, and
    /// so does a reader reverted to the raw count.
    #[test]
    fn every_reader_of_the_window_asks_the_clamp() {
        const SRC: &str = include_str!("app.rs");
        const HELPERS: [&str; 7] = [
            "looking(",
            "live_cursor(",
            "trail_cursor(",
            "window_of(",
            "snapped(",
            "after_read(",
            "resized_window(",
        ];
        const ALLOWED: [(&str, &str); 12] = [
            (
                "} else if app.back.index() != 0 {",
                "the prediction gate, after the screen gate refused the alternate screen",
            ),
            (
                "&& self.app.back.index() == 0",
                "the slide gate, beside `screen == Screen::Normal`",
            ),
            (
                "let now = i64::try_from(self.app.back.index()).unwrap_or(0);",
                "the wheel over the history: the alternate screen returned arrow keys above",
            ),
            (
                "if back.index() == self.app.back.index() {",
                "the same wheel",
            ),
            ("self.app.back = back;", "the wheel's write"),
            (
                "if back != self.app.back {",
                "the snap, after `snapped` chose",
            ),
            ("self.app.back = back;", "the snap's write"),
            (
                "if back.index() != self.app.back.index() {",
                "the thumb's drag: a thumb is drawn only off a live primary",
            ),
            ("self.app.back = back;", "the drag's write"),
            (
                "self.app.back = Rows::ZERO;",
                "a program's page width: the window to the live screen, a write",
            ),
            (
                "let window = self.app.back;",
                "the reset's reveal: `restart` reads it only for a reset made from the primary",
            ),
            (
                "let window = self.back;",
                "the restart tests' read, as the pump's",
            ),
        ];
        // Spelled apart so this line does not name the window itself.
        let tokens = [concat!("self.", "back"), concat!("app.", "back")];
        let names_the_window = |line: &str| {
            tokens.iter().any(|tok| {
                line.match_indices(tok).any(|(at, _)| {
                    !line[at + tok.len()..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_')
                })
            })
        };
        let mut raw: Vec<&str> = SRC
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//") && !l.starts_with('"') && !l.starts_with("(\""))
            .filter(|l| names_the_window(l))
            .filter(|l| !HELPERS.iter().any(|h| l.contains(h)))
            .collect();
        raw.sort_unstable();
        let mut allowed: Vec<&str> = ALLOWED.iter().map(|(l, _)| *l).collect();
        allowed.sort_unstable();
        assert_eq!(
            raw, allowed,
            "a raw read of the window that does not ask the clamp"
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
            assert_eq!(snapped(t, back).index(), 0, "on the primary, a key snaps");
            feed(t, b"\x1b[?1049h");
            assert_eq!(
                snapped(t, back).index(),
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
                let before = Before::of(t);
                feed(t, &bytes);
                assert_eq!(
                    after_read(t, Rows::of_count(3), before),
                    Rows::ZERO,
                    "{what}: the window is not on the live screen"
                );
            });
        }
        // The control: a read with no erase carries the window up the
        // history with the lines it shows.
        with_history(|t| {
            let before = Before::of(t);
            feed(t, &lines(2));
            assert_eq!(after_read(t, Rows::of_count(3), before).index(), 5);
        });
    }

    /// **From the alternate screen it is the primary's window that goes
    /// live**, the one `snapped` keeps while a program has the screen: the
    /// erase emptied the primary's history under it.
    #[test]
    fn an_erase_on_the_alternate_screen_puts_the_primarys_window_live() {
        with_history(|t| {
            let mut back = Rows::of_count(3);
            let read = |t: &mut Term<'_>, bytes: &[u8], back: &mut Rows| {
                let before = Before::of(t);
                feed(t, bytes);
                *back = after_read(t, *back, before);
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

/// **What a full reset owes the app** (note V, V1, the owner's 2026-09-25
/// answer): when the core's reset count moves across a read, the selection
/// goes, the echoes drawn ahead of the shell are dropped as contradicted, the
/// wheel's part-line settles, and a window over the history comes to the live
/// screen with its thumb revealed -- and no screen-switch fade runs, because
/// a reset on the primary puts no other surface up. The window and the slide
/// already follow `Term::history_erasures`, which `RIS` moves; these hold
/// what only the reset count can say, and `CSI 3 J` -- which moves the
/// erasure count and keeps the screen -- is the control that tells the two
/// apart. Each read here is the pump's: `Before`, the feed, `after_read`,
/// then `restart`.
#[cfg(test)]
mod restart_tests {
    use super::{Before, Guess, Kept, Prediction, Surface, Thumb, after_read, restart, window_of};
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
        back: Rows,
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
                back: Rows::ZERO,
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
            let before = Before::watched(t);
            let window = self.back;
            feed(t, bytes);
            self.back = after_read(t, self.back, before);
            restart(
                t,
                before,
                window,
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
                assert_eq!(s.back, Rows::ZERO, "read size {size}");
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
            s.back = Rows::of_count(5);
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
            s.back = Rows::of_count(5);
            assert_eq!(s.read(t, b"\x1bc"), (Surface::Same, Thumb::Reveal));
            assert_eq!(s.back, Rows::ZERO, "the control: it came live");
        });
        with_history(|t| {
            let mut s = State::new();
            assert_eq!(s.read(t, b"\x1bc").1, Thumb::Leave, "from the live screen");
        });
        with_history(|t| {
            let mut s = State::new();
            s.back = Rows::of_count(5);
            assert_eq!(s.read(t, b"\x1b[3J").1, Thumb::Leave, "on ED 3");
            assert_eq!(s.back, Rows::ZERO, "the control: ED 3 brought it live");
        });
        with_history(|t| {
            let mut s = State::new();
            s.back = Rows::of_count(5);
            s.read(t, b"\x1b[?1049h");
            assert_eq!(s.back.index(), 5, "the control: the primary's window kept");
            assert_eq!(s.read(t, b"\x1bc"), (Surface::Switched, Thumb::Leave));
            assert_eq!(t.screen(), Screen::Normal);
            assert_eq!(s.back, Rows::ZERO);
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
            s.back = back;
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
    /// whole, then cut in two at every byte; from a window five lines up,
    /// and from the live screen, the control, where nothing reveals; one
    /// row after an erasure in a pump before, so the watch must be armed at
    /// each pump's reading and not left where the terminal began. The
    /// rows pin the answer built at 232e1ab for a pump of its own: the
    /// reset reveals when the window it took live was the primary's and
    /// over the history at that byte (V1's first fork), so only the first
    /// erasure in a pump can, since any erasure takes the window live.
    #[test]
    fn a_resets_reveal_is_the_same_however_the_bytes_fall_into_pumps() {
        let lines: Vec<u8> = (0..12)
            .flat_map(|i| format!("after {i}\r\n").into_bytes())
            .collect();
        let refill = [&b"\x1b[?1049h\x1bc"[..], &lines, b"\x1bc"].concat();
        let erased = [&b"\x1b[3J"[..], &lines].concat();
        let rows: [(&str, &[u8], &[u8], bool); 11] = [
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
                assert_eq!(
                    reveals(back, setup, &[bytes]),
                    canon,
                    "{what}, from {from}, whole"
                );
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

    /// **What a reset's reveal shows**, through the frame's own bar
    /// (`baud_render::bar`, the overlay style, at the frame's widths): the
    /// thumb a key's snap would show is revealed, but `RIS` empties the
    /// history and a window with nothing behind it has no knob, so nothing
    /// is drawn at the reset; lines printed within the hold give a knob at
    /// the bottom of its travel until the fade ends. That is what HEAD
    /// draws, pinned here because the reveal's test read only `Thumb`.
    /// What an emptied history shows is Jev's answer, as built (A, decisions
    /// lines 453-454): V1's fork says reveal "on that snap", the scrollbar
    /// note draws no thumb with no history, and Apple's `flashScrollers`
    /// text permits a flash on new content without requiring one. The controls: the
    /// instrument draws a knob over the history before the reset, and with
    /// the reveal left out the lines show none.
    #[test]
    fn a_resets_reveal_draws_a_knob_only_over_lines_printed_after_it() {
        let size = (Px::of(800), Px::of(500));
        let knob = |t: &Term<'_>, back: Rows, rv: Reveal, now: HostTime| {
            let s = Scroll {
                window: window_of(t, back),
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
                s.back = Rows::of_count(5);
                let mut shown = Reveal::HIDDEN;
                shown.reveal(t0);
                assert!(knob(t, s.back, shown, t0).is_some(), "the control");
                let mut rv = Reveal::HIDDEN;
                if s.read(t, b"\x1bc").1 == Thumb::Reveal && revealed {
                    rv.reveal(at(0));
                }
                let sample = |t: &Term<'_>, back: Rows, rv: &mut Reveal, ms: u64| {
                    rv.step(at(ms));
                    knob(t, back, *rv, at(ms))
                };
                assert_eq!(t.scrollback().len(), 0, "the reset emptied it");
                if revealed {
                    assert!(rv.look(at(10)).shown.get() > 0, "the thumb is revealed");
                }
                assert!(
                    sample(t, s.back, &mut rv, 10).is_none(),
                    "no knob at the reset (as built, and Jev's answer A)"
                );
                assert!(sample(t, s.back, &mut rv, 90).is_none());
                s.read(t, &lines);
                assert_eq!(t.scrollback().len(), 6, "the lines refilled it");
                for ms in [110, 400, 700] {
                    assert_eq!(
                        sample(t, s.back, &mut rv, ms).is_some(),
                        revealed,
                        "a knob over the lines at {ms} ms, revealed: {revealed}"
                    );
                }
                assert!(sample(t, s.back, &mut rv, 800).is_none(), "faded");
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
    /// so the source is held: the erasure watch is armed and the window read
    /// before the feed;
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
        assert!(at("let before = Before::watched(&mut self.app.term);") < at("let window = "));
        // Spelled apart, so the window's clamp test does not read it as a
        // read of the window.
        let window = concat!("let window = self.app", ".back;");
        assert!(at(window) < at("crate::reads::feed("));
        assert!(at("Ring::Restored(_) => self.app.selection = None,") < call);
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
    use baud_render::{Rect, Shape, TrailLook};
    use std::time::{Duration, Instant};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 10,
    };
    const CELL: (f32, f32) = (10.0, 20.0);

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
        assert_eq!(live_cursor(&t, Rows::ZERO), None, "the program hid it");
        let (r, c) = cell!(1, 7);
        assert_eq!(
            trail_cursor(&t, Rows::ZERO),
            Some(TrailCursor::Placed(r, c))
        );
        feed(&mut t, frame(cell!(8, 7)).as_bytes());
        let (r, c) = cell!(8, 7);
        assert_eq!(
            trail_cursor(&t, Rows::ZERO),
            Some(TrailCursor::Placed(r, c)),
            "ge"
        );
        // A hidden cursor no program placed is followed by nothing.
        feed(&mut t, b"\x1b[5;5Hab\x1b[?25l");
        assert_eq!(trail_cursor(&t, Rows::ZERO), None, "a hide after text");
        // Shown, it is the drawn cursor again.
        feed(&mut t, b"\x1b[?25h");
        let (r, c) = cell!(4, 6);
        assert_eq!(live_cursor(&t, Rows::ZERO), Some((r, c)));
        assert_eq!(trail_cursor(&t, Rows::ZERO), Some(TrailCursor::Shown(r, c)));
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
            trail_cursor(&t, Rows::ZERO),
            Some(TrailCursor::Placed(r, c)),
            "the control: placed on the live screen"
        );
        assert_eq!(trail_cursor(&t, Rows::of_count(3)), None);
    }

    /// **On the alternate screen the window is the live screen**, whatever
    /// the primary's window was left at -- `looking`'s clamp, which the source
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
        let back = Rows::of_count(3);
        assert_eq!(
            trail_cursor(&t, back),
            None,
            "the control: looking back on the primary"
        );
        feed(&mut t, b"\x1b[?1049h");
        feed(&mut t, frame(cell!(1, 7)).as_bytes());
        let (r, c) = cell!(1, 7);
        assert_eq!(trail_cursor(&t, back), Some(TrailCursor::Placed(r, c)));
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
            let now = trail_cursor(t, Rows::ZERO).map(|f| block(f.cell()));
            let old = live_cursor(t, Rows::ZERO).map(block);
            (
                chase.frame(now, at(ms), CELL).is_some(),
                before.frame(old, at(ms), CELL).is_some(),
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
            trail_cursor(&t, Rows::ZERO),
            Some(TrailCursor::Placed(r, c)),
            "the control: l placed the cursor a column on"
        );
        assert_eq!(seen(&t, 4100), (false, false), "l first seen");
        assert_eq!(seen(&t, 4120), (false, false), "l snaps");
    }

    /// **A fire keeps burning at a placed cursor**, where a hidden one puts
    /// it out: helix hides its cursor for good, so under the old rule a fire
    /// on `ge` could not even light, and a placement is a cursor to burn at.
    #[test]
    fn a_fire_burns_at_a_placed_cursor() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut chase: Chase<Instant> = Chase::new(Style::Effect(Effect::Fire));
        let mut fire = |t: &Term<'_>, ms: u64| {
            let now = trail_cursor(t, Rows::ZERO).map(|f| block(f.cell()));
            matches!(chase.frame(now, at(ms), CELL), Some(TrailLook::Vfx(_)))
        };
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
        // A hide that places nothing puts it out, as a hidden cursor does.
        feed(&mut t, b"\x1b[5;5Hab\x1b[?25l");
        assert!(!fire(&t, 210), "a cursor not placed puts it out");
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
        use super::{Path, chase_style};
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
            for path in [Path::Cpu, Path::Instances, Path::Cells] {
                let case = format!("{trail:?} on {path:?}");
                let mut owned = Owned::new(&ws);
                let mut t = Term::new(owned.storage(), &ws);
                let mut chase: Chase<Instant> = Chase::new(chase_style(trail.style(), path));
                let t0 = Instant::now();
                let (mut rows, mut wheel, mut from_landed) = (Vec::new(), false, 0);
                let mut last: Option<(TrailLook, Rect)> = None;
                for (k, bytes) in frames.iter().enumerate() {
                    feed(&mut t, bytes);
                    let cell = trail_cursor(&t, Rows::ZERO).map(|f| f.cell());
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
                        let rect = trail_cursor(&t, Rows::ZERO).map(|f| block(f.cell()));
                        let look = chase.frame(rect, now, CELL);
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
                if trail == CursorTrail::Fire && path != Path::Cpu {
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
    /// primary and on the cell path: every one of the sixteen combinations,
    /// and the lift is the pan under `ride` and nothing under `stay`.
    #[test]
    fn the_grid_slides_only_where_every_gate_says_so() {
        for scroll in [ScrollMode::Smooth, ScrollMode::Normal] {
            for window in [0, 3] {
                for screen in [Screen::Normal, Screen::Alternate] {
                    for drawn in [SlideDrawn::Cells, SlideDrawn::Other] {
                        let want = scroll == ScrollMode::Smooth
                            && window == 0
                            && screen == Screen::Normal
                            && drawn == SlideDrawn::Cells;
                        assert_eq!(
                            slides(scroll, Rows::of_count(window), screen, drawn),
                            want,
                            "{scroll:?} {window} {screen:?} {drawn:?}"
                        );
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
    /// one place, straight after the slide retires the lines, on the
    /// display link's clock the trail's frame reads.
    #[test]
    fn the_pan_is_read_once_and_the_ride_follows_the_retire() {
        const SRC: &str = include_str!("app.rs");
        let pan_read = concat!("self.", "slide.pan()");
        assert_eq!(SRC.matches(pan_read).count(), 1, "one read of the pan");
        let ride = concat!(".chase", ".ride(");
        assert_eq!(SRC.matches(ride).count(), 1, "one ride of the chase");
        let retire = concat!("self.app.slide", ".retire(n);");
        let at = SRC.find(retire).expect("the retire");
        let next = SRC[at + retire.len()..].trim_start();
        assert!(
            next.starts_with(concat!("self.ride", "_trail(n, screen);")),
            "the ride is taken straight after the retire"
        );
        let body = &SRC[SRC.find(concat!("fn ride", "_trail(")).expect("ride_trail")..];
        let body = &body[..body.find("\n    }\n").expect("its end")];
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
    //! Reduce Motion: the owner's rule, "no trail, unless the config names
    //! one", and blinking text held shown. The decisions are pure and gated
    //! here; `poll_motion`, the tick and the launch read are AppKit-bound,
    //! held by the source gates at the end.
    use super::{
        MotionSource, Owes, Path, chase_style, motion_change, motion_override, retrail,
        system_motion,
    };
    use crate::config::{CursorTrail as T, SystemMotion as M, TrailPick as P};
    use crate::trail::Chase;
    use baud_domain::{Col, HostTime, Row};
    use baud_render::Rect;
    use std::cell::Cell;

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
        let mut chase: Chase<HostTime> = Chase::new(chase_style(T::Fire.style(), Path::Cells));
        chase.frame(Some(cell(5.0, 3.0)), at(1.0), (10.0, 20.0));
        chase.frame(Some(cell(60.0, 3.0)), at(1.008), (10.0, 20.0));
        assert!(
            chase
                .frame(Some(cell(60.0, 3.0)), at(1.020), (10.0, 20.0))
                .is_some(),
            "the control: a fire is lit"
        );
        let (mut ghost, mut last) = (None, Some((Row::ZERO, Col::ZERO)));
        let owes = retrail(
            &mut chase,
            &mut ghost,
            &mut last,
            T::Off,
            Path::Cells,
            Some(at(1.021)),
        );
        assert_eq!(owes, Owes::Frame, "one frame to clear the fire");
        assert_eq!(last, None, "the cursor's last cell is forgotten");
        for k in 0..1200u32 {
            let now = at(1.021 + f64::from(k) / 120.0);
            assert_eq!(chase.owes(now), Owes::Nothing, "tick {k}");
        }
    }

    /// A switch at rest owes no frame: nothing was drawn.
    #[test]
    fn a_switch_at_rest_owes_no_frame() {
        let mut chase: Chase<HostTime> = Chase::new(chase_style(T::Fire.style(), Path::Cells));
        chase.frame(Some(cell(5.0, 3.0)), at(1.0), (10.0, 20.0));
        let (mut ghost, mut last) = (None, None);
        let owes = retrail(
            &mut chase,
            &mut ghost,
            &mut last,
            T::Off,
            Path::Cells,
            Some(at(2.0)),
        );
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
        const SHIM_FN: &str = concat!("    fn ring_", "bell(&mut self) {");
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
            concat!(".ring_", "bell()"),
            "NSBeep(",
            "requestUserAttention(",
            "orderFrontRegardless(",
        ] {
            assert_eq!(code.matches(call).count(), 1, "{call}");
        }
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
            r#"crate::reads::feed(
                term,
                drained.chunks.iter().map(|chunk| {
                    if let Some(t) = trace.as_mut() { t.fed(&chunk.bytes); }
                    &chunk.bytes[..]
                }),
                |b| outbox.push(b),
                |t| {
                    pasteboard("#,
            r#"clips.poll(t, osc52));
                    resized = honour(widths.poll(t, host), t).or(resized);
                },
            );
            self.app.pty.follow("#,
            r#"self.app.term.input_encoding());
            if let Some(ws) = resized { self.program_resized(ws); }
            if let Some(line) = self.app.clips
                .core_refusals(&self.app.term, self.app.clipboard.budget)
            { crate::warn!("{line}"); }
            if !drained.chunks.is_empty() { self.app.pending = Pending::Dirty; }
            if self.app.ringer.hear(&self.app.term, Instant::now(), self.app.interval)
                == Rings::Ring
            { self.ring_bell(); }"#
        );
        let span = bare(READ_TO_RING);
        let fed = pump.find("crate::reads::feed(").expect("the read");
        assert!(pump[fed..].starts_with(&span), "the read, then the ring");
        let rung = fed + span.len();
        let first_return = fed + pump[fed..].find("return;").expect("the control: a return");
        let held = pump
            .find(&bare(
                ".observe(&self.app.term, Instant::now()) == Update::Honoured",
            ))
            .expect("the held update");
        assert!(rung < first_return, "before the clean-frame return");
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
        use super::{BLINK_HALF, Blink, BlinkPhases, RAPID_HALF, text_blink};
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

    /// **Each rate blinks at its own rate**: across a minute of the display
    /// link's clock, sampled every millisecond, `SGR 5` hides 75 times and
    /// `SGR 6` 150 -- `[[source: ecma48]]` § 8.3.117's "less then 150 per
    /// minute" and "150 per minute or more" -- each shown for half its cycle.
    #[test]
    fn each_blink_rate_flips_at_its_own_rate() {
        use super::{Blink, BlinkPhases, text_blink};
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
        let call = concat!("let phase = text_", "blink(target, self.app.motion);");
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
}

#[cfg(test)]
mod anchor_tests {
    use super::anchored;
    use baud_domain::Rows;

    use super::Ring;

    // Spelled out rather than wrapped in a two-line constructor: one
    // taking a bare count would be a raw integer in a signature, which
    // the audit counts and whose budget is not moved to fit a change.
    // (Nor may this comment name one -- the audit reads comments too,
    // which is written down in CLAUDE.md and cost a run here anyway.)
    #[test]
    fn a_window_on_the_live_screen_stays_there_however_far_the_ring_moved() {
        // The zoom-in case: a shorter screen retired twenty-four rows.
        assert_eq!(
            anchored(
                Rows::ZERO,
                Ring::Retired(Rows::of_count(24)),
                Rows::of_count(400)
            )
            .index(),
            0
        );
        // And the zoom-out case, where they came back.
        assert_eq!(
            anchored(
                Rows::ZERO,
                Ring::Restored(Rows::of_count(24)),
                Rows::of_count(400)
            )
            .index(),
            0
        );
    }

    #[test]
    fn a_window_looking_back_follows_the_ring_and_is_bounded_by_it() {
        let ten = Rows::of_count(10);
        assert_eq!(
            anchored(ten, Ring::Retired(Rows::of_count(24)), Rows::of_count(400)).index(),
            34
        );
        assert_eq!(
            anchored(ten, Ring::Restored(Rows::of_count(24)), Rows::of_count(400)).index(),
            0
        );
        // Never past what is retained, and never below the live screen.
        assert_eq!(
            anchored(ten, Ring::Retired(Rows::of_count(1000)), Rows::of_count(40)).index(),
            40
        );
        assert_eq!(
            anchored(
                ten,
                Ring::Restored(Rows::of_count(1000)),
                Rows::of_count(40)
            )
            .index(),
            0
        );
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
/// worst case was one stray prompt; the owner's screen showed five. A
/// trace *is* the session, so a test replaying it is a test of what
/// happened.
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
}

#[derive(Clone, Copy)]
struct BarDrag {
    grab_y: Px,
    top: Px,
}

/// Whether a relayout drew a frame of its own, so a caller that wants one
/// can tell whether it still has to ask.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Drew {
    Yes,
    No,
}

/// A key's echo drawn before the pty answered (row 9 of note N,
/// `docs/research/notes/q-local-echo.md`): the cell it stands in on the
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

/// The hold after a read, and the zone to remember for the next: a prompt
/// opening -- the zone turning to `Prompt` -- releases a hold a contradicted
/// prediction set. **The primary's zone, not the live screen's.** Each
/// screen has its own, so a full-screen program a line-editor widget opens
/// with no `C` turned the live zone to `Output` and back, and its closing
/// read as a prompt opening. The primary's stays `Prompt` across it, and on
/// the primary the two are one.
fn hold_after(term: &Term<'_>, hold: Hold, last: Zone) -> (Hold, Zone) {
    let zone = term.primary_zone();
    let hold = match zone == Zone::Prompt && last != Zone::Prompt {
        true => Hold::Open,
        false => hold,
    };
    (hold, zone)
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

/// Whether the scrollbar took a mouse event.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Took {
    Yes,
    No,
}

/// Where a pointer event goes once the scrollbar and links have passed it:
/// to the child, which tracks the mouse, or to the selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum To {
    Child,
    Select,
}

/// Whether Shift is held at a pointer event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shift {
    Held,
    Up,
}

/// Who owns the press in progress, decided at the press. `Free` between
/// presses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owner {
    Free,
    Pressed(To),
}

/// **Where one pointer event goes, and who owns the press after it.** The
/// child's when it tracks the mouse, unless Shift is held, which is a
/// selection whatever the child asked for: xterm's default, ctlseqs'
/// XTSHIFTESCAPE at 0. **Decided at the press and kept to its release**, as
/// the scrollbar's grab and a link's arming are: Shift pressed or let go
/// mid-drag moves neither the drag nor the release, so the child never sees
/// a press without its release -- under `?1001` that release is the one
/// that ends the drawn region and sends the report (`[[source: ctlseqs]]`,
/// "until button release, and then sends the program the release
/// coordinates") -- and a selection never loses the release that ends it.
/// A move with no button held has no press to follow and is decided as it
/// comes, as is a drag or release whose press nothing here saw. A second
/// button pressed mid-drag decides again, so a press whose release was lost
/// cannot hold the next gesture.
fn pointer_route(
    owner: Owner,
    kind: ViewMouseKind,
    tracking: MouseTracking,
    shift: Shift,
) -> (To, Owner) {
    let now = match (tracking, shift) {
        (MouseTracking::Off, _) | (_, Shift::Held) => To::Select,
        _ => To::Child,
    };
    match (kind, owner) {
        (ViewMouseKind::Down, _) => (now, Owner::Pressed(now)),
        (ViewMouseKind::Dragged, Owner::Pressed(to)) => (to, owner),
        (ViewMouseKind::Up, Owner::Pressed(to)) => (to, Owner::Free),
        (ViewMouseKind::Up, Owner::Free) => (now, Owner::Free),
        _ => (now, owner),
    }
}

/// `pointer_route`: the press decides, and Shift mid-drag does not.
#[cfg(test)]
mod route_tests {
    use super::{Owner, Shift, To, ViewMouseKind as K, pointer_route};
    use baud_domain::MouseTracking;

    /// Feed `events` (kind, Shift held) from `Free`, and say where each went.
    fn routed(tracking: MouseTracking, events: &[(K, Shift)]) -> (Vec<To>, Owner) {
        let mut owner = Owner::Free;
        let mut to = Vec::new();
        for &(kind, shift) in events {
            let (t, next) = pointer_route(owner, kind, tracking, shift);
            to.push(t);
            owner = next;
        }
        (to, owner)
    }

    /// **Shift pressed mid-drag keeps the release the child's.** Under
    /// `?1001` that release ends the drawn region and sends the report; per
    /// event, the selection took it and the region stayed drawn.
    #[test]
    fn shift_pressed_mid_drag_leaves_the_release_with_the_child() {
        let (to, owner) = routed(
            MouseTracking::Highlight,
            &[
                (K::Down, Shift::Up),
                (K::Dragged, Shift::Held),
                (K::Up, Shift::Held),
            ],
        );
        assert_eq!(to, [To::Child, To::Child, To::Child]);
        assert_eq!(owner, Owner::Free, "the release frees it");
    }

    /// **Shift let go mid-drag keeps a selection the selection's**, to the
    /// release that ends it.
    #[test]
    fn shift_let_go_mid_drag_leaves_the_selection_its_release() {
        let (to, owner) = routed(
            MouseTracking::Normal,
            &[
                (K::Down, Shift::Held),
                (K::Dragged, Shift::Up),
                (K::Up, Shift::Up),
            ],
        );
        assert_eq!(to, [To::Select, To::Select, To::Select]);
        assert_eq!(owner, Owner::Free);
    }

    /// **Tracking switched on or off mid-drag moves nothing either**: the
    /// press decided.
    #[test]
    fn a_tracking_change_mid_drag_leaves_the_press_s_owner() {
        let mut owner = Owner::Free;
        let (t, next) = pointer_route(owner, K::Down, MouseTracking::Off, Shift::Up);
        owner = next;
        assert_eq!(t, To::Select);
        let (t, next) = pointer_route(owner, K::Up, MouseTracking::Normal, Shift::Up);
        assert_eq!((t, next), (To::Select, Owner::Free));
    }

    /// **A move with no press is decided as it comes**, so `?1003` hover
    /// reaches the child and Shift hover does not, and it leaves the owner.
    #[test]
    fn a_move_with_no_press_is_decided_as_it_comes() {
        for (shift, want) in [(Shift::Up, To::Child), (Shift::Held, To::Select)] {
            let (t, next) = pointer_route(Owner::Free, K::Moved, MouseTracking::AnyEvent, shift);
            assert_eq!((t, next), (want, Owner::Free));
        }
    }

    /// **A press decides again**, even over an owner whose release was lost.
    #[test]
    fn a_press_decides_again() {
        let stale = Owner::Pressed(To::Child);
        let (t, next) = pointer_route(stale, K::Down, MouseTracking::Normal, Shift::Held);
        assert_eq!((t, next), (To::Select, Owner::Pressed(To::Select)));
    }
}

impl Driver {
    /// A key the user pressed: sent to the core and on to the child, with
    /// the instrument's first two stamps around it -- the key event's own
    /// timestamp and this responder running, then the `write(2)` having
    /// returned. The wheel on the alternate screen sends arrow keys through
    /// `App::send` directly: it is not a keystroke and is not stamped.
    fn typed(&mut self, e: KeyEvent) {
        let e = KeyEvent {
            kind: std::mem::take(&mut self.key_kind),
            ..e
        };
        self.app.latency.key(current_event_time, host_now);
        let (text, held) = (e.text, e.mods);
        match self.app.send(e) {
            Sent::Bytes => {
                self.app.latency.written(host_now);
                self.app.echo = Echo::Wanted;
                if let Some(c) = text {
                    self.predict(c, held);
                }
            }
            Sent::Nothing => self.app.latency.unsent(),
        }
        // A prediction is owed its frame now, not at the next wake: the
        // keystroke is the event, and nothing else pumps the loop for it.
        if self.app.guess == Guess::Owed {
            self.pump(Trigger::Wake);
        }
    }

    /// Row 9: a printable typed at the shell's prompt is drawn before the
    /// pty answers, as an overlay the next read judges. The gates are the
    /// design's (`docs/research/notes/q-local-echo.md`): the core's prompt
    /// zone, the primary screen, the live view, no synchronised update, a
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
        let refused = if held != Modifiers::NONE {
            // A control or option chord carries text -- the unmodified
            // character -- and `send` writes the *chord*, so the glyph
            // would not be the byte. Only the input manager's `text` path,
            // which builds its events at `Modifiers::NONE`, predicts.
            Some("mods")
        } else if app.hold != Hold::Open {
            Some("hold")
        } else if app.term.zone() != Zone::Prompt {
            Some("zone")
        } else if app.term.screen() != Screen::Normal {
            Some("screen")
        } else if app.back.index() != 0 {
            Some("back")
        } else if app.term.synchronized_output() {
            Some("sync")
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
                let content = view.row(p.row).get(p.col.index()).map(|c| c.content);
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
        (app.hold, app.last_zone) = hold_after(&app.term, app.hold, app.last_zone);
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
        if let Trigger::Tick = trigger {
            self.poll_focus();
            self.poll_motion();
        }
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
            }
        }
        self.apply_style();
        let drained = self.app.pty.drain();
        if !drained.chunks.is_empty() {
            startup_first("first bytes", &FIRST_BYTES);
            // The reader stamped each read as it returned: the first one
            // after a key's write is that key's echo.
            self.app
                .latency
                .echoed(drained.chunks.iter().map(|chunk| chunk.at));
        }
        let before = Before::watched(&mut self.app.term);
        // The primary's window before the read, for the thumb a reset
        // reveals.
        let window = self.app.back;
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
        let (term, trace, outbox, clips, widths) = (
            &mut self.app.term,
            &mut self.app.trace,
            &mut self.app.outbox,
            &mut self.app.clips,
            &mut self.app.widths,
        );
        let osc52 = self.app.clipboard.osc52;
        let host = self.app.sizing.host();
        let mut resized = None;
        crate::reads::feed(
            term,
            drained.chunks.iter().map(|chunk| {
                if let Some(t) = trace.as_mut() {
                    t.fed(&chunk.bytes);
                }
                &chunk.bytes[..]
            }),
            |b| outbox.push(b),
            |t| {
                pasteboard(clips.poll(t, osc52));
                resized = honour(widths.poll(t, host), t).or(resized);
            },
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
        if !drained.chunks.is_empty() {
            self.app.pending = Pending::Dirty;
        }
        // **The bells this read brought ring now**, before the early returns
        // for a held update or a busy frame: a sound, a bounce and a raise
        // are not drawn, and a minimized window draws nothing. Once a frame
        // interval (`Ringer`).
        if self
            .app
            .ringer
            .hear(&self.app.term, Instant::now(), self.app.interval)
            == Rings::Ring
        {
            self.ring_bell();
        }
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
            // Output shows the cursor and restarts its blink: foot's "prevent
            // blinking while typing" (`caret.rs`).
            self.app.caret.restart(host_now());
            // The first read after a key's write is that key's echo -- the
            // instrument's rule, and the loop's: it is owed a frame now.
            if self.app.echo == Echo::Wanted {
                self.app.echo = Echo::Fed;
            }
            self.judge();
            // A program's title: only a read can set one, so this is the
            // one place it can have changed.
            if let Some(t) = self.titles.next(
                self.app.term.title(),
                self.app.term.input_encoding(),
                "baud",
            ) {
                self.window.set_title(&t);
            }
        }
        // Looking back while output scrolls: the window stays on the lines
        // it was showing, which means it moves up the history by however
        // many rows the primary retired -- its top line's advance, whatever
        // screen is up -- and no further than what is retained.
        // A read can move the window backwards: a redraw mark moving the
        // window over its block (`prompt.rs`).
        let moved = read_moved(&self.app.term, before, resized);
        self.app.back = after_read(&self.app.term, self.app.back, before);
        // The selection names absolute lines; the count that numbers them
        // advances with the screen. A surface switch is another ring, and a
        // line the ring recycled is clipped away. A window moved *back* has
        // un-retired lines the count already numbered, so the selection is
        // dropped rather than renumbered: it names rows a redraw is about
        // to replace.
        let screen = self.app.term.screen();
        // Rows a copy moved in place: their selection names rows whose text
        // went elsewhere, and no count renumbers it.
        if self.app.selection.as_ref().is_some_and(|s| {
            stale_in_moved_rows(s, &self.app.term, before, self.app.scrolled, moved)
        }) {
            self.app.selection = None;
        }
        match moved {
            Ring::Retired(n) => {
                // Numbered before the count advances: the rows under a
                // region were those lines before this read.
                if self.app.selection.as_ref().is_some_and(|s| {
                    stale_under_region(s, &self.app.term, before, self.app.scrolled)
                }) {
                    self.app.selection = None;
                }
                self.app.scrolled = self.app.scrolled.advanced(n);
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
                    && self.app.back.index() == 0
                    && screen == Screen::Normal
                    && self.app.cell.is_some()
                {
                    self.app.slide.retire(n);
                    self.ride_trail(n, screen);
                }
            }
            Ring::Restored(_) => self.app.selection = None,
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
            window,
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
            // The alternate screen swapped: fade the new one in from the
            // background, off `tick_target` the way the trail is spawned.
            if self.app.screen_transition == crate::config::ScreenTransition::On {
                self.app.transition = Some(TransitionState {
                    start: self.app.tick_target,
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
            self.app.selection = None;
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
        // opens and stalls -- or a program that dies inside one -- would
        // freeze the window until something closes it. The closing `?2026l`
        // feeds in and wakes us, and this call then presents the finished
        // screen.
        // Each update is timed from its own opening: a close and a reopen in
        // one read restart the clock (`UpdateClock`), where an instant kept
        // across them let the new update inherit the old one's start.
        if self.app.updates.observe(&self.app.term, Instant::now()) == Update::Honoured {
            // Honoured: what the update changes of `?5` is its to hide.
            self.app.bell.hold(&self.app.term);
            return;
        }
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
        let due = trigger == Trigger::Tick
            || self.app.echo == Echo::Fed
            || self.app.guess == Guess::Owed
            || self.app.marked == Marked::Owed
            || self.app.last_commit.elapsed() >= self.app.interval;
        if due && !busy {
            let view = self.view.clone();
            self.app.present(&view, Show::Event);
        }
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
        if key.modifiers.command {
            // Menu territory: a key equivalent the menu carries never gets
            // here, and one it does not sends nothing to the child.
            return Handled::Consumed;
        }
        self.snap_to_live();
        if composing_first(key, &self.app.composing) {
            return Handled::Interpret;
        }
        let m = mods(key.modifiers);
        let e = if let Some(code) = special(key) {
            KeyEvent::press(code, m)
        } else if key.modifiers.control || key.modifiers.option {
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
            e
        } else {
            // The input manager will call `text` or `command`, which stamp
            // the keystroke themselves; a record opened here as well would
            // be one no write ever completes.
            return Handled::Interpret;
        };
        self.typed(e);
        Handled::Consumed
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
            // One record per character: a paste through the input manager
            // is one `text` and as many keys as it has characters.
            self.typed(e);
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
            "zoomIn:" => {
                let at = match self.deferred {
                    Deferred::Zoom(points) => points,
                    _ => self.app.font_size,
                };
                return self.zoom(at + 1.0);
            }
            "zoomOut:" => {
                let at = match self.deferred {
                    Deferred::Zoom(points) => points,
                    _ => self.app.font_size,
                };
                return self.zoom(at - 1.0);
            }
            "zoomActual:" => return self.zoom(self.app.font_base),
            _ => {}
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
        self.typed(KeyEvent::press(code, m));
    }

    /// The input manager's composition changed: drawn at the cursor now,
    /// not at the next read, and gone when it is committed or cancelled --
    /// the crate calls this with "" before it hands the committed text to
    /// `text`.
    fn marked_text(&mut self, text: &str) {
        let composing: Vec<Codepoint> = text
            .chars()
            .filter_map(|c| Codepoint::new(u32::from(c)))
            .collect();
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
        self.poll_focus();
        // Any press is input to the blink's stop -- one the scrollbar, a
        // link or a tracking child takes too -- before those take it.
        if event.kind == ViewMouseKind::Down {
            self.app.caret.quiet(host_now());
        }
        if self.scrollbar(event) == Took::Yes {
            return;
        }
        // A Cmd press over an `OSC 8` link is the terminal's, the whole
        // gesture -- before the child's tracking, like Shift below, so a
        // child never sees a release without its press.
        if self.links(event) == Took::Yes {
            return;
        }
        // The child's, when it tracks the mouse -- unless Shift is held,
        // which is a selection whatever the child asked for: xterm's
        // default, ctlseqs' XTSHIFTESCAPE at 0. Decided at the press, so
        // Shift mid-drag cannot hand the release to the other side.
        let (to, owner) = pointer_route(
            self.owner,
            event.kind,
            self.app.term.mouse_tracking(),
            match event.modifiers.shift {
                true => Shift::Held,
                false => Shift::Up,
            },
        );
        self.owner = owner;
        if to == To::Select {
            self.select(event);
            return;
        }
        let (row, col, px) = self.pointer_under(event.x, event.y);
        let pressed = match event.button {
            ViewButton::Left => MouseButton::Left,
            ViewButton::Middle => MouseButton::Middle,
            ViewButton::Right => MouseButton::Right,
            ViewButton::Other(_) => MouseButton::None,
        };
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
            self.app.flush_reply();
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
        match crate::wheel::route(&self.app.term) {
            Route::Report => {
                let at = self.pointer_under(event.x, event.y);
                // Every report into the outbox as it is queued -- the core's
                // buffer holds 21 of them -- and then one write: what the
                // kernel cannot take now stays owed to the next pump.
                let outbox = &mut self.app.outbox;
                crate::wheel::report(&mut self.app.term, lines, at, |b| outbox.push(b));
                self.app.flush_reply();
                return;
            }
            Route::Keys => {
                for key in crate::wheel::keys(lines) {
                    self.app.send(key);
                }
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
        let retained = i64::try_from(self.app.term.scrollback().len()).unwrap_or(i64::MAX);
        let now = i64::try_from(self.app.back.index()).unwrap_or(0);
        let back = Rows::of_count(usize::try_from((now + step).clamp(0, retained)).unwrap_or(0));
        if back.index() == self.app.back.index() {
            return;
        }
        self.app.back = back;
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
        // then nothing flips.
        let phase = text_blink(target, self.app.motion);
        if phase != self.app.blink {
            if self.app.blink_present.flips(self.app.blink, phase) {
                self.app.next_damage = Damage::Full;
                self.app.pending = Pending::Dirty;
            }
            self.app.blink = phase;
        }
        // The cursor's blink owes a frame at each flip, and when it stops in
        // the off phase -- focus lost, a steady style, hidden, looked away
        // from -- the frame that shows it (`caret.rs`). Nothing while it does
        // not blink, so an idle steady cursor builds no frame.
        if let Some(now) = HostTime::of_secs(target) {
            let blinks = crate::caret::blinks(
                self.app.term.cursor_style(),
                self.app.focus,
                live_cursor(&self.app.term, self.app.back),
            );
            if self.app.caret.due(now, blinks) == Owes::Frame {
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
            let back = looking(&self.app.term, self.app.back);
            let drag = match self.bar_drag {
                Some(_) => Drag::Dragging,
                None => Drag::Idle,
            };
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
        self.pump(Trigger::Tick);
        // Every tick, not every frame: an idle screen builds no frame and
        // is the one this has to be able to report on.
        self.app.report();
        // After the pump: a record shown since the last announcement takes
        // this target as its early bound, and one that already has its
        // early bound takes it as the late one.
        self.app.latency.tick(|| HostTime::of_secs(target));
        startup_first("first tick done", &FIRST_TICK_DONE);
    }

    /// The system's light/dark flipped, or the window moved under a
    /// different appearance. Recorded rather than applied: this lands with
    /// the view's borrow held, and swapping the palette repaints.
    fn appearance(&mut self, appearance: Appearance) {
        self.style.0.set(Some(appearance));
    }

    fn presented(&mut self) {
        self.app.presented();
    }

    fn wake(&mut self) {
        if self.app.frames.value() > 0
            && self.app.presenter.as_ref().map(Presenter::in_flight) != Some(InFlight::Busy)
        {
            startup_first("first completion", &FIRST_DONE);
        }
        self.pump(Trigger::Wake);
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

/// Where the Dock's icon comes from: the bundle's `CFBundleIconFile` when
/// the binary runs inside an `.app`, else set from the embedded image.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DockIcon {
    Bundled,
    Ours,
}

/// baud's icon, rasterised from `assets/logo/` and embedded, so it needs no
/// bundle. The Dock wants a proper app icon -- a rounded square with the mark
/// centred and padding around it, like every other app; the titlebar wants
/// the bare mark snug against its box, or the strip makes it tiny -- so the
/// two read from different files.
fn icon() -> Option<Retained<NSImage>> {
    // A proper app icon for the Dock: a dark rounded square at ~80% of the
    // canvas with the amber mark centred and transparent padding around it,
    // the way every other Dock icon sits. The titlebar uses the bare mark
    // (`icon_titlebar`); the Dock wants the box. The SVG itself, not a raster,
    // so NSImage renders it as a vector.
    const BYTES: &[u8] = include_bytes!("../../assets/logo/baud-icon-dock.svg");
    macos_liquid_glass::image_from_bytes(BYTES)
}

/// The same mark, cropped snug to its own bounds. In the padded 512 box the
/// glyph is a third of the height, so a titlebar-sized box drew a mark half
/// the size of a traffic light; this viewBox is tight to the strokes, so the
/// box the icon is given is the size the mark is drawn at.
fn icon_titlebar() -> Option<Retained<NSImage>> {
    const BYTES: &[u8] = include_bytes!("../../assets/logo/baud-mark-amber-tight.svg");
    macos_liquid_glass::image_from_bytes(BYTES)
}

/// **Below macOS 26 baud refuses, and says why.** The owner, 2026-10-13,
/// picked "macOS 26 and later": the window is NSGlassEffectView, which
/// exists from 26, and there is no fallback backdrop. The refusal has three
/// layers. Launch Services refuses the bundle, from Info.plist's
/// `LSMinimumSystemVersion` of 26.0. A bare `baud` run from a shell meets
/// no such check -- macOS 13's dyld and kernel do not refuse on the
/// binary's `minos` -- so it reaches this, which prints `GlassError`'s
/// message on stderr and exits 1 where it used to panic on
/// `.expect("glass")`. It runs first in [`run`], ahead of the display link
/// and `-[NSApplication activate]` (macOS 14) and ahead of the shell's
/// spawn, so nothing is started that it would then abandon.
fn refusal(glass: Glass) -> Option<String> {
    match glass {
        Glass::Present => None,
        Glass::Absent => Some(refusal_message(&GlassError::Unsupported)),
    }
}

/// Whether this system has `NSGlassEffectView`, the window's material.
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
/// release ("Liquid Glass requires macOS 26 or later").
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
    let size = NSSize::new(960.0, 600.0);
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
    let inset = {
        let content = NSRect::new(NSPoint::new(0.0, 0.0), size);
        let frame =
            NSWindow::frameRectForContentRect_styleMask(content, NSWindowStyleMask::Titled, mtm);
        (frame.size.height - size.height).max(0.0)
    };
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
    let boot = Boot::prepare(scale);
    // The gutter in points: the grid floats inset from all four sides -- below
    // the titlebar and in from the left, right and bottom -- with the glass in
    // the margin. The pty and the renderer follow the inset area.
    let padding = boot.padding;
    let theme_choice = boot.theme;
    let glass_cfg = boot.glass;
    let floor = boot.floor;
    let halves = boot.halves;
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
    let ws = grid.as_ref().map_or_else(
        || {
            grid_for(
                NSSize::new(
                    (size.width - 2.0 * padding).max(0.0),
                    (size.height - inset - 2.0 * padding).max(0.0),
                ),
                scale,
                (cw, ch),
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
    let pty = Pty::spawn(&ws, cell.cell_w, cell.cell_h, move || {
        if let Some(w) = wake_late.get() {
            w.wake();
        }
    })
    .expect("pty");
    startup_mark("pty");
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
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
    let window = GlassWindow::new(mtm, size, "baud");
    startup_mark("glass window");
    // The inset predicted above, checked against the window that exists
    // now; the window's own value is what the layout uses.
    let measured = {
        let w = window.ns_window();
        let content_h = w
            .contentView()
            .map_or(size.height, |v| v.frame().size.height);
        (content_h - w.contentLayoutRect().size.height).max(0.0)
    };
    if (measured - inset).abs() > 0.5 {
        startup_note(&format!(
            "titlebar inset: predicted {inset:.1}, measured {measured:.1}"
        ));
    }
    let inset = measured;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
    // **The material is the config's, and it is chosen once.** `Regular` is
    // the standard frosted material and `Clear` the thinner, more transparent
    // one, which Apple's HIG pairs with a dimming layer over bright content.
    // baud *is* that dimming layer -- it paints its palette's background over
    // the material -- so asking for the thicker one and then covering it at
    // 85%, which is what a fixed `background_alpha = 216` did, was paying for
    // a material twice and seeing neither. Unlike the dim, the tint and the
    // shadow, the style does not change with the appearance, so there is no
    // setter call anywhere: it is a constructor argument and nothing else.
    let glass = GlassSurface::new(
        mtm,
        frame,
        match glass_cfg.style {
            crate::config::Material::Clear => GlassStyle::Clear,
            crate::config::Material::Regular => GlassStyle::Regular,
        },
        16.0,
    )
    .unwrap_or_else(|e| refuse(refusal_message(&e)));
    // Tinted before it is shown rather than after: the surface is not the
    // window's content until the first frame is on the layer, so doing it
    // here costs nothing and there is no frame in which an untinted
    // material is visible.
    apply_glass(&glass, &window, &glass_cfg, material_appearance(&window));
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
        let wanted = NSSize::new(
            f64::from(ws.ws_col) * cw / scale + 2.0 * padding + f64::from(bar.value()) / scale,
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
    // The icon in the titlebar strip is placed after `show()`, below,
    // once the traffic-light buttons have been laid out.
    // The container is the window's content until the first tick, when
    // it moves under the glass; see `LateGlass`.
    window.set_content_view(&container);
    startup_mark("views");
    window.set_accepts_mouse_moved(true);
    window.set_min_size(NSSize::new(200.0, 100.0));
    window.center();
    startup_mark("centered");
    window.show();
    startup_mark("shown");
    let mut icon_iv: Option<Retained<NSImageView>> = None;
    let (mut icon_x, mut icon_s) = (0.0_f64, 0.0_f64);
    if let Some(img) = &icon {
        // Placed against the real traffic lights rather than a hand-picked x.
        // The window is FullSizeContentView, so `container` (the content view)
        // spans the whole frame and shares the theme frame's bottom-left
        // origin -- so a standard button's `frame` is already in container
        // coordinates, no conversion (which would be `unsafe` here) needed.
        let w = window.ns_window();
        let lights = (
            w.standardWindowButton(NSWindowButton::CloseButton),
            w.standardWindowButton(NSWindowButton::MiniaturizeButton),
            w.standardWindowButton(NSWindowButton::ZoomButton),
        );
        // Read after `show()`, so the buttons are laid out; a zero-height
        // frame means they are not, and the fallback places it by the strip.
        let measured = match lights {
            (Some(close), Some(mini), Some(zoom)) => {
                let (c, m, z) = (close.frame(), mini.frame(), zoom.frame());
                (z.size.height >= 1.0).then(|| {
                    // Horizontal comes from the buttons: x is shared with the
                    // container (the titlebar view spans the width from x=0).
                    // Vertical does NOT -- a button's `frame` is in the
                    // titlebar view's own coordinates, so its y is near that
                    // view's bottom, not the window's top. Convert would be
                    // `unsafe`, forbidden here, so the icon is centred in the
                    // strip from geometry we already know: it runs from
                    // `full.height - inset` to `full.height`.
                    let gap = (m.origin.x - (c.origin.x + c.size.width)).max(0.0);
                    // A peer of the lights: a little larger than a button, the
                    // snug mark reading at about a traffic light's size.
                    let s = (z.size.height * 1.4).clamp(16.0, inset - 4.0);
                    (s, z.origin.x + z.size.width + gap)
                })
            }
            _ => None,
        };
        let (s, left) = measured.unwrap_or_else(|| {
            let s = (inset - 2.0).clamp(18.0, 28.0);
            (s, 74.0)
        });
        // Centred in the titlebar strip, whose top is the window top.
        let cy = full.height - inset / 2.0;
        // The snug crop, not the padded dock mark, so the box is the size.
        let iv = match icon_titlebar() {
            Some(tight) => macos_liquid_glass::icon_view(mtm, &tight),
            None => macos_liquid_glass::icon_view(mtm, img),
        };
        iv.setFrame(NSRect::new(
            NSPoint::new(left, cy - s / 2.0),
            NSSize::new(s, s),
        ));
        // No autoresizing mask: a drag to a tiny window clamps the frame and
        // it never recovers. `Driver::place_icon` re-places it every resize.
        container.addSubview(&iv);
        icon_iv = Some(iv);
        icon_x = left;
        icon_s = s;
    }
    // The cell the observer writes and `pump` reads; cloned into both.
    let style = PendingStyle::default();
    let mut driver = Driver {
        app: probe,
        view: view.clone(),
        dock_icon: match dock {
            DockIcon::Bundled => None,
            DockIcon::Ours => icon.clone(),
        },
        held: None,
        key_kind: KeyEventKind::Press,
        armed: None,
        bar_drag: None,
        grab: Grab::Free,
        owner: Owner::Free,
        deferred: Deferred::Nothing,
        style: style.clone(),
        theme_choice,
        glass_cfg,
        themes: Themes::of(&glass_cfg, floor, &halves),
        palette_for: None,
        glass: glass.clone(),
        window: window.clone(),
        titles: crate::title::Shown::new(),
        icon: icon_iv,
        icon_super: container.clone(),
        icon_x,
        icon_size: icon_s,
        icon_inset: inset,
    };
    // The first frame now, on this thread, before the run loop is entered:
    // the geometry the view's first resize callback would report, then one
    // draw. Its completion wakes the responder through the run loop, which
    // is where the surface reaches the layer.
    // **The palette for the first frame, from the window rather than the
    // observer.** The observer below fires once the moment it is built and
    // would set this -- but it is built after the glass, which is after
    // this frame, so a light-mode launch would draw one dark frame and
    // correct it. The window knows its own appearance already.
    if theme_choice == crate::config::ThemeChoice::Auto {
        let appearance = appearance_of(&window);
        driver.palette_for = Some(appearance);
        let theme = driver.themes.get(appearance).clone();
        driver.app.renderer.set_theme(theme);
        crate::host::tell_defaults(
            &mut driver.app.term,
            driver.app.renderer.theme(),
            baud_core::Overrides::Keep,
        );
    }
    Responder::resized(&mut driver, view.frame().size, scale);
    let v = driver.view.clone();
    driver.app.present(&v, Show::Now);
    startup_mark("first frame");
    // The glass under the terminal now that the first frame is on the
    // layer: ordering a window whose content is a glass effect view costs
    // 10-20 ms more than ordering one whose content is a plain view, so
    // the window went up bare and the material lands here, before the run
    // loop -- and before a responder exists, because moving the container
    // re-enters the view's resize callback, which a responder callback
    // holding the view's borrow cannot survive.
    window.set_content_view(glass.view());
    glass.set_content_view(&container);
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
    {
        let glass_cb = glass.clone();
        let window_cb = window.clone();
        let observer = macos_liquid_glass::icon_style::StyleObserver::new(mtm, move |_| {
            apply_glass(
                &glass_cb,
                &window_cb,
                &glass_cfg,
                material_appearance(&window_cb),
            );
        });
        // The window is never torn down before the process ends, so the
        // observer outlives every reference by outliving the process --
        // the alternative is a field on a driver that is about to be boxed
        // into the view, which buys nothing a leak does not.
        core::mem::forget(observer);
        startup_mark("style observer");
    }
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
    // The menu after the first frame, before the run loop: key equivalents
    // need it from the first event, and nothing on screen waits for it.
    macos_liquid_glass::menu::install_main_menu(&ns_app, mtm, "baud");
    startup_mark("menu");
    view.set_responder(Box::new(driver));
    view.start_display_link();
    view.waker().wake();
    ns_app.activate();
    ns_app.run();
}

/// The font sizes a zoom may reach, in points: below six nothing reads,
/// and past seventy-two a cell is most of a small window.
const ZOOM_MIN: f32 = 6.0;
const ZOOM_MAX: f32 = 72.0;

impl Driver {
    /// Typing, and a paste, snap the window back to the live screen, where
    /// the keystroke's effect is.
    fn snap_to_live(&mut self) {
        let back = snapped(&self.app.term, self.app.back);
        if back != self.app.back {
            self.app.back = back;
            self.app.travel.settle();
            self.app.pending = Pending::Dirty;
            // A programmatic scroll, which AppKit reveals: the thumb shows
            // at the bottom and fades.
            if let Some(now) = HostTime::of_secs(self.app.tick_target) {
                self.app.reveal.reveal(now);
            }
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

    /// **The bell's AppKit shim**: the window as AppKit has it now -- key
    /// (`[[source: apple-nswindow-iskeywindow]]`), minimized (`[[source:
    /// apple-nswindow-isminiaturized]]`) or neither, asked here rather than
    /// taken from the tick's focus poll, which may not have run since the
    /// window changed -- classed by [`Window::of`](crate::bell::Window::of),
    /// then [`ring`](crate::bell::ring)'s decision, and the calls it names.
    /// Both are tested; this is the two questions and the four calls, each
    /// under its condition, and the source gate holds it to that text.
    fn ring_bell(&mut self) {
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

    /// **Output retired `n` lines under a smooth scroll**: what the trail
    /// drew from the text rides up with it, where the grid slides and the
    /// config says `ride` -- the quad or sweep in flight for at most two
    /// rows, an effect's whole path, the ghost's origin (`trail.rs`,
    /// `ride_ghost`). On the display link's clock, which the trail's frame
    /// reads, and never past one window above the top.
    fn ride_trail(&mut self, n: Rows, screen: Screen) {
        let drawn = SlideDrawn::of(
            self.app.presenter.as_ref().map(Presenter::mode),
            self.app.cell.as_ref(),
        );
        if self.app.trail_scroll != crate::config::TrailScroll::Ride
            || !slides(
                self.app.scroll,
                looking(&self.app.term, self.app.back),
                screen,
                drawn,
            )
            || self.app.slide.pan().value() == 0
        {
            return;
        }
        let cell_h = self.app.metrics_cell().1 as f32;
        let rows = Rows::of_count(usize::from(self.app.grid.ws_row));
        let Some((dy, floor)) = ride_px(n, rows, cell_h) else {
            return;
        };
        if let Some(now) = HostTime::of_secs(self.app.tick_target) {
            self.app.chase.ride(dy, now, floor, cell_h);
        }
        let tick = self.app.tick_target;
        self.app.trail = self.app.trail.and_then(|st| {
            let p = ((tick - st.start) / TRAIL_S) as f32;
            ride_ghost(st, dy, p, cell_h)
        });
    }

    /// Whether the system asks for reduced motion, read on the tick like
    /// focus: one message send and a cached byte, about 0.04 us.
    /// **Polled, not observed**: AppKit registers its own observer of the
    /// display-options notification in `sharedApplication`, and its handler
    /// invalidates the cached answer before anything is posted, so a toggle
    /// reads fresh here within a frame (the design's probes; the live
    /// toggle is the owner's check). Under `BAUD_MOTION` nothing is read.
    fn poll_motion(&mut self) {
        let m = system_motion(self.app.motion_source, reduce_motion);
        if m == self.app.motion {
            return;
        }
        self.app.motion = m;
        let Some(next) = motion_change(self.app.trail_pick, self.app.cursor_trail, m) else {
            return;
        };
        crate::info!("motion: {m:?}; trail: {next:?}");
        let owes = retrail(
            &mut self.app.chase,
            &mut self.app.trail,
            &mut self.app.last_cursor_cell,
            next,
            self.app.wanted,
            HostTime::of_secs(self.app.tick_target),
        );
        self.app.cursor_trail = next;
        if owes == Owes::Frame {
            self.app.pending = Pending::Dirty;
        }
    }

    /// Record a focus transition and report it if the application asked.
    /// Only a *change* reaches the core, so polling cannot repeat `CSI I`.
    fn set_focus(&mut self, f: baud_core::Focus) {
        if self.app.focus == f {
            return;
        }
        self.app.focus = f;
        if self.app.term.focus(f) {
            self.app.flush_reply();
        }
        // A gain reads the system's caret again -- a change made in System
        // Settings while baud was in the background lands here -- and is
        // input, so the cursor comes back shown and the stop starts over.
        if f == baud_core::Focus::Gained {
            self.read_caret_system();
            self.app.caret.input(host_now());
        }
    }

    /// The system caret's rate and its non-blinking switch (`caret.rs`),
    /// read on the first tick -- after the first present, so a launch's
    /// first frame does not wait for it, and where a scripted launch, whose
    /// window is never key, still pays and measures it -- and at every focus
    /// gain. Not observed: a change made while baud keeps focus lands at the
    /// next gain.
    fn read_caret_system(&mut self) {
        let start = Instant::now();
        let cycle = os_cycle();
        let switch = if os_at_least_15() {
            crate::pty::prefers_steady_caret()
        } else {
            NonBlinking::Off
        };
        let took = start.elapsed();
        self.app
            .caret
            .set_system(cycle, Gate::of(self.app.blink_choice, switch));
        crate::info!(
            "caret: {cycle}, switch {switch:?}, read in {} us",
            took.as_micros()
        );
        if switch == NonBlinking::On
            && self.app.blink_choice == crate::config::BlinkChoice::On
            && self.app.warned_blink == Done::No
        {
            crate::warn!(
                "baud: [cursor] blink = true overrides the system's non-blinking caret; \
                 blink = \"auto\" honours it"
            );
            self.app.warned_blink = Done::Yes;
        }
    }

    /// `OSC 8` activation: a Cmd press over a link takes the whole gesture
    /// for the terminal, and its release over the same link opens it. The
    /// rules are `crate::link::gesture`'s; this maps the view's event onto
    /// them and the answer onto the driver's state.
    fn links(&mut self, event: &Mouse) -> Took {
        use crate::link::{Chord, Clicks, Gesture, Press};
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
        let under = {
            // The cell the pointer is over in the *view* -- scrolled back
            // included -- read the way `select` reads it; a wide tail
            // carries its lead's link, so either half resolves.
            let view = self.app.term.view(looking(&self.app.term, self.app.back));
            let id = view.row(row)[col.index()].flags.link();
            self.app.term.link_uri(id).map(|_| id)
        };
        match crate::link::gesture(press, chord, clicks, self.app.focus, self.armed, under) {
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
            Gesture::Open(id) => {
                self.armed = None;
                self.open_link(id);
                Took::Yes
            }
        }
    }

    /// Hand a link to the OS, or refuse it and say why. The one call site
    /// of the opener, and it holds a `Target` the policy built rather than
    /// a string, so a URI the policy did not admit cannot reach it.
    fn open_link(&mut self, id: baud_domain::LinkId) {
        use crate::link::{Action, Host};
        use baud_present_macos::link::{self as opener, Opened};
        let Some(uri) = self.app.term.link_uri(id) else {
            // The table never frees a slot, so a link that resolved at the
            // press resolves at the release; counted rather than trusted.
            self.app.links.refused.bump();
            crate::warn!("link refused (gone)");
            return;
        };
        let shown = uri
            .iter()
            .take(80)
            .copied()
            .collect::<Vec<u8>>()
            .escape_ascii()
            .to_string();
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
        let (row, col) = self.cell_under(event.x, event.y);
        let scrolled = self.app.scrolled;
        let back = looking(&self.app.term, self.app.back);
        let view = self.app.term.view(back);
        let top = scrolled.top(back);
        let line = scrolled.line(back, row);
        let span_at = |grain: Grain| match grain {
            Grain::Cell => Span::cell(view.row(row), line, col),
            Grain::Word => Span::word(view.row(row), line, col),
            Grain::Line => Span::line(&view, top, row),
        };
        match event.kind {
            ViewMouseKind::Down if event.button == ViewButton::Left => {
                let grain = match event.clicks {
                    0 | 1 => Grain::Cell,
                    2 => Grain::Word,
                    _ => Grain::Line,
                };
                self.app.selection = Some(Selection::pressed(
                    self.app.selection,
                    press_of(event.modifiers),
                    grain,
                    span_at,
                ));
                self.held = Some(MouseButton::Left);
            }
            ViewMouseKind::Dragged if self.held == Some(MouseButton::Left) => {
                if let Some(sel) = self.app.selection.as_mut() {
                    sel.extend(span_at(sel.grain()));
                }
            }
            ViewMouseKind::Up => {
                self.held = None;
                if self.app.selection.is_some_and(|s| s.is_click()) {
                    self.app.selection = None;
                }
            }
            _ => return,
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
        let scale = self.app.scale;
        let at = |v: f64| Px::of(u32::try_from((v * scale).max(0.0) as i64).unwrap_or(0));
        // The frame is centred in the surface; the thumb's geometry is in
        // frame pixels, so the pointer comes in from view pixels less the
        // letterbox margin.
        let (px, py) = self.app.pad_px;
        let x = Px::of(at(event.x).value().saturating_sub(px.value()));
        let y = Px::of(at(event.y).value().saturating_sub(py.value()));
        let win = self.app.window();
        let size = self.app.renderer.frame_size();
        let cell_h = self.app.renderer.metrics().cell_h;
        let widths = crate::reveal::widths(crate::reveal::Scale(scale));
        let scroller = self.app.scroller;
        let strip = place(x, size.0, widths);
        self.app.pointer = match event.kind {
            ViewMouseKind::Exited => Place::Grid,
            _ => strip,
        };
        let Some(now) = HostTime::of_secs(self.app.tick_target) else {
            return Took::No;
        };
        // The hover, on every event but a drag's, which holds its own; the
        // legacy scroller has none.
        if self.bar_drag.is_none() && scroller == Scroller::Overlay {
            let motion = match event.kind {
                ViewMouseKind::Exited => Motion::Leaving,
                _ => Motion::Within,
            };
            let over = crate::reveal::over(strip, win.back, motion);
            let before = self.app.reveal;
            self.app.reveal.hover(over, now);
            if self.app.reveal != before {
                self.app.pending = Pending::Dirty;
            }
        }
        // A press is tested against what is on the screen: the look the
        // last frame drew, not the one the hover update above will draw.
        let look = self.app.drawn_look;
        let knob = thumb(win, size, cell_h, widths, look.hover);
        let top = match event.kind {
            ViewMouseKind::Down => {
                let button = match event.button {
                    ViewButton::Left => BarButton::Left,
                    _ => BarButton::Other,
                };
                let on = knob.map(|t| match t.covers_y(y) {
                    true => Knob::On,
                    false => Knob::Off,
                });
                let takes = crate::reveal::press(scroller, strip, look.shown, on, button);
                self.grab = takes.grabs();
                match (takes, knob) {
                    (Takes::Cells, _) => return Took::No,
                    (Takes::Drag, Some(t)) => {
                        self.bar_drag = Some(BarDrag {
                            grab_y: y,
                            top: t.top,
                        });
                        if scroller == Scroller::Overlay {
                            self.app.reveal.hover(Over::Strip, now);
                        }
                        return Took::Yes;
                    }
                    (Takes::Jump, Some(t)) => {
                        Px::of(y.value().saturating_sub(t.height.value() / 2))
                    }
                    _ => return Took::Yes,
                }
            }
            ViewMouseKind::Dragged => {
                let Some(d) = self.bar_drag else {
                    // A press the scrollbar took that is not a drag -- a
                    // jump, or the gutter's -- keeps its drags too.
                    return match self.grab {
                        Grab::Bar => Took::Yes,
                        Grab::Free => Took::No,
                    };
                };
                // The window can no longer scroll -- the alternate screen
                // came up mid-drag, which keeps no history -- so the drag
                // ends rather than write the primary's window from a
                // geometry with nothing behind it. The press stays taken
                // until its release.
                if win.retained.index() == 0 {
                    self.bar_drag = None;
                    return Took::Yes;
                }
                let moved = i64::from(y.value()) - i64::from(d.grab_y.value());
                Px::of(u32::try_from((i64::from(d.top.value()) + moved).max(0)).unwrap_or(0))
            }
            ViewMouseKind::Up => {
                let grabbed = core::mem::replace(&mut self.grab, Grab::Free);
                if self.bar_drag.take().is_some() && scroller == Scroller::Overlay {
                    // The end of a drag: still over the strip it stays
                    // widened, anywhere else it is a reveal that holds and
                    // fades.
                    let over = crate::reveal::over(strip, win.back, Motion::Within);
                    self.app.reveal.hover(over, now);
                    self.app.reveal.reveal(now);
                    self.app.pending = Pending::Dirty;
                    self.pump(Trigger::Wake);
                }
                return match grabbed {
                    Grab::Bar => Took::Yes,
                    Grab::Free => Took::No,
                };
            }
            // Motion with nothing pressed: the legacy gutter keeps it from
            // a program tracking every motion.
            ViewMouseKind::Moved => {
                return match crate::reveal::motion(scroller, strip) {
                    Takes::Cells => Took::No,
                    _ => Took::Yes,
                };
            }
            _ => return Took::No,
        };
        let back = back_for(win, size, cell_h, top);
        self.app.reveal.reveal(now);
        if back.index() != self.app.back.index() {
            self.app.back = back;
            // Dragging the thumb into history settles a slide, as the wheel
            // does: it names live-screen rows the scrolled-back view lacks.
            if back.index() != 0 {
                self.app.slide.snap();
            }
            self.app.pending = Pending::Dirty;
            self.pump(Trigger::Wake);
        }
        Took::Yes
    }

    /// Re-place the titlebar mark at the top-left of its superview, whose
    /// height it reads now rather than at launch. Cheap -- one `setFrame` --
    /// and it runs on every resize so the mark cannot drift or vanish.
    fn place_icon(&self) {
        let Some(iv) = &self.icon else { return };
        let cy = self.icon_super.frame().size.height - self.icon_inset / 2.0;
        iv.setFrame(NSRect::new(
            NSPoint::new(self.icon_x, cy - self.icon_size / 2.0),
            NSSize::new(self.icon_size, self.icon_size),
        ));
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
        self.app.predictions.clear();
        self.app.guess = Guess::None;
        self.app.slide.snap();
        self.app.back = Rows::ZERO;
        if let Some(t) = self.app.trace.as_mut() {
            t.resized(grid.ws());
        }
        self.app.tell_pty();
        self.app.grid = ws;
        self.app.selection = None;
        self.app.next_damage = Damage::Full;
        self.app.pending = Pending::Dirty;
        let scale = self.app.scale;
        let bar = gutter(self.app.scroller, crate::reveal::widths(Scale(scale)));
        let (cw, _) = self.app.metrics_cell();
        let width =
            crate::width::content_width(grid, cw, scale, self.app.padding, f64::from(bar.value()));
        widen_later(self.window.ns_window(), width);
    }

    /// The grid for a view of `size` points at `scale`, and the terminal,
    /// the child and the frame following it -- on a resize, and on a zoom.
    fn relayout(&mut self, size: NSSize, scale: CGFloat) -> Drew {
        // **A resize no longer waits for the frame in flight, and that is
        // the freeze the owner reported.** It used to: a reflow rewrites
        // the ring, and while the shader read the ring in place that was
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
        // The titlebar mark follows the window's top-left on every resize.
        self.place_icon();
        // The scroller the setting asks for now, and the gutter the frame
        // keeps for it -- before either early return below, because a frame
        // that changed width is a new frame whether or not the grid did.
        self.app.scroller = scroller_style();
        let bar = gutter(self.app.scroller, crate::reveal::widths(Scale(scale)));
        let reframed = self.app.renderer.frame_size().0.value()
            != self.app.renderer.pixel_size().0.value() + bar.value();
        self.app.renderer.set_gutter(bar);
        if reframed {
            self.app.next_damage = Damage::Full;
            self.app.pending = Pending::Dirty;
        }
        if self.app.sizing == Sizing::Fixed {
            // The grid is pinned, and a zoom still moves the pixels.
            self.app.tell_pty();
            return Drew::No;
        }
        let ws = grid_in(
            (px_w, px_h),
            self.app.padding * scale,
            f64::from(bar.value()),
            (cw, ch),
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
            // Anchor a window that is *already* looking back; leave one on
            // the live screen where it is. See `anchored`. `scrolled` is
            // left untouched: it numbers lines for a selection, and a
            // resize has already cleared the selection.
            self.app.back = resized_window(&mut self.app.term, &ws, self.app.back);
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
            // no longer name cells.
            self.app.selection = None;
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
            // is still busy this step draws nothing and the in-flight frame's
            // completion drives the next; either way the main thread returns
            // at once and keeps tracking the drag. `term.resize` kept the
            // content -- the pitch is fixed, each row clipped or extended in
            // place -- and the child redraws into it on `SIGWINCH`.
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
    fn zoom(&mut self, points: f32) {
        let points = points.clamp(ZOOM_MIN, ZOOM_MAX);
        if std::env::var_os("BAUD_DEBUG_RESIZE").is_some() {
            eprintln!(
                "zoom: asked {points}, at {}, busy {:?}",
                self.app.font_size,
                self.app.presenter.as_ref().map(Presenter::in_flight)
            );
        }
        if points == self.app.font_size {
            return;
        }
        // The rescale itself is safe mid-flight now: the glyph map is built
        // over the other buffer and the atlas builds a replaced sheet
        // afresh, so neither writes what a live draw is reading. What still
        // waits is that a zoom *re-lays the grid*, and a reflow rewrites the
        // ring the shader reads. The two have to land together -- a frame
        // drawn at the new cell size against the old column count is neither
        // size -- so the whole step waits for the frame that is reading.
        if self.app.presenter.as_ref().map(Presenter::reshape) == Some(Reshape::Wait) {
            self.deferred = Deferred::Zoom(points);
            return;
        }
        let scale = self.app.scale;
        let Some(px) = PxSize::of(points * scale as f32) else {
            return;
        };
        if self.app.rescale(px).is_none() {
            return;
        }
        self.app.font_size = points;
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
            material_appearance(&self.window),
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
        self.app
            .renderer
            .set_theme(self.themes.get(appearance).clone());
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

    /// Cmd-C: the selection's text to the pasteboard, if there is one.
    fn copy(&mut self) {
        let Some(sel) = self.app.selection else {
            return;
        };
        let text = sel.text(&self.app.term, self.app.scrolled);
        if !text.is_empty() {
            clipboard::write_text(&text);
        }
    }

    /// Cmd-V: the pasteboard's string to the child, bracketed when it
    /// asked (`?2004`), through the channel keys take.
    fn paste(&mut self) {
        let Some(text) = clipboard::read_text() else {
            return;
        };
        let bracket = if self.app.term.bracketed_paste() {
            Bracket::On
        } else {
            Bracket::Off
        };
        self.snap_to_live();
        self.app.caret.input(host_now());
        // Through the outbox, behind anything owed and ahead of anything
        // typed after it: a paste longer than the slave's queue -- 1022
        // bytes on Darwin -- waited in `write(2)` for the child to read it.
        self.app.flush_reply();
        self.app.outbox.push(&paste_bytes(&text, bracket));
        self.app.flush_reply();
    }

    /// The cell under a point in the view, clamped to the grid: the view
    /// is the terminal's area below the titlebar inset, in points, and the
    /// grid is in pixels of the cell size.
    fn cell_under(&self, x: f64, y: f64) -> (Row, Col) {
        let (scale, cell, grid, pad) = self.pointer_geometry();
        cell_under((x, y), scale, cell, grid, pad)
    }

    /// The cell under a view point and the pixel it is at: what a mouse
    /// report needs, `?1016`'s pixel included.
    fn pointer_under(&self, x: f64, y: f64) -> (Row, Col, PointerPx) {
        let (scale, cell, grid, pad) = self.pointer_geometry();
        pointer_under((x, y), scale, cell, grid, pad)
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

/// Whether the child asked for bracketed paste.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bracket {
    On,
    Off,
}

/// The bytes a paste sends: bracketed as `?2004` specifies, with a forged
/// end inside the text removed, because the child would take it as the
/// end; or, unbracketed, with line endings as typed input carries them.
fn paste_bytes(text: &str, bracket: Bracket) -> Vec<u8> {
    match bracket {
        Bracket::On => {
            let body = text.replace("\x1b[201~", "");
            [b"\x1b[200~".as_slice(), body.as_bytes(), b"\x1b[201~"].concat()
        }
        Bracket::Off => text.replace("\r\n", "\r").replace('\n', "\r").into_bytes(),
    }
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

/// The cell under a point **and the pixel it is at, from one computation**,
/// so the two cannot name different places: `?1016` reports the pixel where
/// every other encoding reports the cell, and an application divides one by
/// the cell size to get the other.
///
/// The pixel is the point in device pixels -- the unit `Term::set_cell_size`
/// is given, and so the unit `CSI 16 t` and `?2048` answer in -- less the
/// padding, zero-based from the grid's first pixel. It is clamped to the
/// text area the way the cell is clamped to the grid, so a pointer over the
/// padding or past the last cell reports the edge pixel of the edge cell.
/// xterm does not clamp: it reports `event->y - OriginY(screen)` whatever it
/// is (`EditorButton`, `[[source: xterm-button]]`), which over the border is
/// zero or negative and on the wire a `0` or a `-1` -- a position no cell
/// has, and one this cell would contradict.
///
/// The cell is the pixel divided by the cell size. It was the point divided
/// by the cell size before the pixel existed, and for a whole-pixel cell,
/// which the renderer's metrics always are, the two are the same number.
fn pointer_under(
    at: (f64, f64),
    scale: CGFloat,
    cell: (f64, f64),
    grid: (u16, u16),
    pad: (u32, u32),
) -> (Row, Col, PointerPx) {
    let axis = |p: f64, pad: u32, cell: f64, cells: u16| {
        let extent = (cell * f64::from(cells)).floor() as i64;
        let px = ((p * scale - f64::from(pad)).floor() as i64).clamp(0, (extent - 1).max(0));
        let index =
            ((px as f64 / cell).floor() as i64).clamp(0, i64::from(cells).saturating_sub(1).max(0));
        (
            usize::try_from(index).unwrap_or(0),
            ImagePx::of(u16::try_from(px).unwrap_or(u16::MAX)),
        )
    };
    let (col, x) = axis(at.0, pad.0, cell.0, grid.0);
    let (row, y) = axis(at.1, pad.1, cell.1, grid.1);
    (Row::from_index(row), Col::at_index(col), PointerPx { x, y })
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// **One place drops.** `Overrides::Drop` appears once in the app's code
    /// outside its tests, in `apply_style`, behind `palette_switch`; and the
    /// flip takes its theme from the prebuilt pair, building none.
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
        let drops = |s: &str| s.matches("Overrides::Drop").count();
        assert!(app.len() > 100_000, "the gate reads the app's code");
        assert_eq!((drops(app), drops(host), drops(linux)), (1, 0, 0));
        let at = app.find("fn apply_style(").expect("apply_style");
        let body = &app[at..at + app[at..].find("\n    }\n").expect("its end")];
        assert!(
            body.contains("Overrides::Drop"),
            "the drop is in apply_style"
        );
        assert!(body.contains("palette_switch("), "behind the switch rule");
        assert!(!body.contains("theme_for("), "the flip builds no theme");
    }

    /// **An effect on the CPU path runs as the quad**, which is what that
    /// path draws of it, so the tick owes no frames for an effect nothing
    /// draws; on either GPU path it is itself, and so is every other trail.
    #[test]
    fn an_effect_on_the_cpu_path_runs_as_the_quad() {
        use crate::trail::Style;
        use baud_render::Shape;
        use baud_render::effect::Effect;
        for e in Effect::ALL {
            let fx = Style::Effect(e);
            assert_eq!(
                chase_style(fx, Path::Cpu),
                Style::Smear(Shape::Quad),
                "{e:?}"
            );
            assert_eq!(chase_style(fx, Path::Cells), fx, "{e:?}");
            assert_eq!(chase_style(fx, Path::Instances), fx, "{e:?}");
        }
        for path in [Path::Cpu, Path::Cells] {
            let sweep = Style::Smear(Shape::Sweep);
            assert_eq!(chase_style(sweep, path), sweep);
        }
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
        let due = &pump[pump.find("let due = ").expect("pump's due")..];
        let due = &due[..due.find(';').expect("its end")];
        assert!(
            due.contains(concat!("self.app.marked == ", "Marked::Owed")),
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

    #[test]
    fn a_paste_is_bracketed_when_asked_and_its_line_endings_are_typed_otherwise() {
        let forged = "a\x1b[201~b\r\nc\n";
        assert_eq!(
            paste_bytes(forged, Bracket::On),
            b"\x1b[200~ab\r\nc\n\x1b[201~".to_vec()
        );
        assert_eq!(
            paste_bytes(forged, Bracket::Off),
            b"a\x1b[201~b\rc\r".to_vec()
        );
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
        let before = Before::watched(&mut t);
        let mut widths = Widths::new(&t);
        let mut resized = None;
        crate::reads::feed(
            &mut t,
            [read.as_slice()],
            |_| {},
            |t| {
                resized = honour(widths.poll(t, Host::Window), t).or(resized);
            },
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
        let run = code.split_once("\npub fn run() {").expect("run").1;
        let run = &run[..run.find("\n}\n").expect("run ends")];
        assert!(run.contains(call), "the launch sizing");
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
            .split_once(concat!("crate::reads::", "feed("))
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

/// **Whether the frame draws a smooth scroll's slide**: smooth scrolling,
/// the window at the live screen, the primary -- the alternate screen keeps
/// no ring and has no leaving row -- and the cell path. The one predicate
/// the trail's ride and its lift both take.
fn slides(
    scroll: crate::config::ScrollMode,
    window: Rows,
    screen: Screen,
    drawn: SlideDrawn,
) -> bool {
    scroll == crate::config::ScrollMode::Smooth
        && window.index() == 0
        && screen == Screen::Normal
        && drawn == SlideDrawn::Cells
}

/// The lift a trail is drawn with: the slide's pan when it rides, and
/// nothing when it stays, which is what it drew before the ride was built.
fn trail_lift(scroll: crate::config::TrailScroll, pan: Px) -> Px {
    match scroll {
        crate::config::TrailScroll::Ride => pan,
        crate::config::TrailScroll::Stay => Px::of(0),
    }
}

/// **How far a trail rides** for `n` retired lines on a screen of `rows`
/// rows, pixels, y down: up by the lines, and never past the floor one
/// window above the top. Nothing for no lines: `pump` sees `Retired` with
/// none on nearly every read, and a ride of nothing would rebase the move
/// in flight on every tick for no change.
fn ride_px(n: Rows, rows: Rows, cell_h: f32) -> Option<(f32, f32)> {
    (n.index() > 0).then(|| {
        (
            -(n.index() as f32 * cell_h),
            -(rows.index() as f32 * cell_h),
        )
    })
}

/// **The ghost rides a scroll** `dy` pixels, y down, at progress `p`: its
/// origin moves by `dy / (1 - p)`, so the ghost drawn at `p` -- a lerp from
/// the origin to the cell it reached -- moves by exactly `dy`. The quad's
/// rules (`Chase::ride`): past [`crate::trail::THRESHOLD`] rows this frame is
/// rigid with the text and the ghost rides no more, finishing its sweep from
/// there; and once it lies wholly above the row that leaves the window,
/// `cell_h` pixels up, nothing of it shows and it is dropped. `None` once it
/// has landed, too.
fn ride_ghost(st: TrailState, dy: f32, p: f32, cell_h: f32) -> Option<TrailState> {
    if p >= 1.0 {
        return None;
    }
    let GhostRide::Riding(r) = st.ride else {
        return Some(st);
    };
    let p = p.max(0.0);
    let origin_dy = st.origin_dy + dy / (1.0 - p);
    let row = |r: Row| r.index() as f32 * cell_h;
    let top = (row(st.from.0) + origin_dy) * (1.0 - p) + row(st.to.0) * p;
    if top + cell_h <= -cell_h - 0.5 {
        return None;
    }
    let ride = if (r + dy).abs() > crate::trail::THRESHOLD * cell_h {
        GhostRide::Folded
    } else {
        GhostRide::Riding(r + dy)
    };
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

/// The system caret's rate: AppKit's own defaults,
/// `NSTextInsertionPointBlinkPeriodOn` and `...Off`, each read on its own
/// as AppKit reads it, with AppKit's 650/350 where one is unset
/// (`caret::Cycle::of_defaults`). `doubleForKey` converts a number or a
/// numeric string, and answers 0 for a key that is absent or will not
/// convert -- which `of_defaults` reads as unset -- by its header.
fn os_cycle() -> Cycle {
    let d = NSUserDefaults::standardUserDefaults();
    Cycle::of_defaults(
        d.doubleForKey(&NSString::from_str("NSTextInsertionPointBlinkPeriodOn")),
        d.doubleForKey(&NSString::from_str("NSTextInsertionPointBlinkPeriodOff")),
        Cycle::APPKIT,
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

/// Where the motion preference comes from: the system, or `BAUD_MOTION`,
/// read once at launch, for a check that must not change the owner's
/// settings -- an instrument like `BAUD_SCROLLER`, not a configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MotionSource {
    System,
    Fixed(crate::config::SystemMotion),
}

/// `BAUD_MOTION`: `reduced` or `full` fix the preference; anything else,
/// or nothing, leaves it to the system.
fn motion_override(v: Option<&str>) -> MotionSource {
    use crate::config::SystemMotion;
    match v {
        Some("reduced") => MotionSource::Fixed(SystemMotion::Reduced),
        Some("full") => MotionSource::Fixed(SystemMotion::Full),
        _ => MotionSource::System,
    }
}

/// The motion preference from `source`, asking the system through `read`
/// only when nothing fixed it.
fn system_motion(source: MotionSource, read: impl FnOnce() -> bool) -> crate::config::SystemMotion {
    use crate::config::SystemMotion;
    match source {
        MotionSource::Fixed(m) => m,
        MotionSource::System => match read() {
            true => SystemMotion::Reduced,
            false => SystemMotion::Full,
        },
    }
}

/// macOS's Reduce Motion, from the window crate's safe getter.
fn reduce_motion() -> bool {
    macos_liquid_glass::accessibility::reduce_motion()
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
    path: Path,
    now: Option<HostTime>,
) -> Owes {
    let drawn = now.is_some_and(|t| chase.owes(t) == Owes::Frame) || ghost.is_some();
    chase.restyle(chase_style(next.style(), path));
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
/// instead, for a check that must not change the owner's settings -- an
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
