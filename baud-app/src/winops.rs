//! What a program's window operation does to the window: XTWINOPS 1-10 and
//! DECSLPP, which the core queues (`Term::window_requests`) and each
//! driver performs. The policy, pure, so its tests run on every platform:
//! the macOS window's frame for each request, and the xdg-shell request,
//! or why there is none.
//!
//! The operations are built, on by default, and
//! `[window] operations = "off"` turns them off in the core. **macOS** does
//! each with AppKit: the frame a move, a resize or a maximise gives the
//! window, measured as XTWINOPS's reports measure it (`window_facts` in
//! `app.rs`), so `CSI 3 ; x ; y t` sends the window to where `CSI 13 t`
//! reports it and `CSI 8 ; r ; c t` gives the grid `r` by `c`. **Linux**
//! does what xdg-shell can: minimise, maximise and restore, full screen,
//! and a size a floating window chooses; the rest has no request there and
//! is counted with its reason.

// Each driver reads its own half.
#![cfg_attr(not(test), allow(dead_code))]

use crate::width::{Grid, Host};
use baud_core::{FullScreen, Maximize, Rows, SizeAsk, WindowOps, WindowRequest};
use baud_render::Tally;

/// A rectangle in AppKit's points: origin at the bottom-left, y up.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn top(self) -> f64 {
        self.y + self.h
    }
}

/// Whether AppKit has the window minimised.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Minimized {
    Yes,
    No,
}

/// Whether the window is in full screen, its style mask's `fullScreen`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Full {
    Yes,
    No,
}

/// The macOS window as AppKit has it when the request is performed.
#[derive(Clone, Copy, Debug)]
pub struct MacWindow {
    /// The frame, title bar included.
    pub frame: Rect,
    /// The content view's size, in points: the grid, the padding on each
    /// side and a legacy scroller's gutter.
    pub content: (f64, f64),
    /// The top of the primary screen's frame, which XTWINOPS's y counts
    /// down from; `None` with no screen.
    pub primary_top: Option<f64>,
    /// The visible frame of the window's screen, the menu bar and the Dock
    /// left out: "the display's height or width", and what a resize is held
    /// inside, as xterm's `limitResize` holds one inside the display.
    pub visible: Option<Rect>,
    pub scale: f64,
    /// One cell, in device pixels.
    pub cell: (f64, f64),
    /// The padding, in points, on each side.
    pub padding: f64,
    /// A legacy scroller's gutter, in device pixels.
    pub gutter: f64,
    pub grid: Grid,
    /// The status line's rows below the grid (`Term::status_rows`): a
    /// resize in cells gives the grid its rows and keeps the line's.
    pub status: Rows,
    pub minimized: Minimized,
    pub full: Full,
    /// The frame a maximise saved, which a restore returns to.
    pub saved: Option<Rect>,
    pub host: Host,
}

/// What a maximise or a restore does to the saved frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Saved {
    Keep,
    Save(Rect),
    Forget,
}

/// Why a request does nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Why {
    /// The window is already as asked: not minimised, not in full screen,
    /// nothing to restore.
    Already,
    /// `BAUD_GRID` pins the grid, so the window keeps its size.
    Pinned,
    /// In full screen the window has the screen; a move, a resize or a
    /// maximise waits for it to leave.
    InFullScreen,
    /// No screen to measure against.
    NoScreen,
}

/// The AppKit call a request makes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MacAction {
    /// `deminiaturize:`.
    Deminiaturize,
    /// `miniaturize:`.
    Miniaturize,
    /// `orderFrontRegardless`, which raises the window "even if its
    /// application isn't active" and takes no focus.
    Front,
    /// `orderBack:`, "without changing either the key window or the main
    /// window".
    Back,
    /// A frame drawn whole, the next.
    Redraw,
    /// `toggleFullScreen:`.
    ToggleFullScreen,
    /// `setFrame:display:`, and what becomes of the saved frame.
    Frame(Rect, Saved),
    Nothing(Why),
}

/// The macOS call for request `r` on window `w`.
pub fn mac(r: WindowRequest, w: &MacWindow) -> MacAction {
    use MacAction::*;
    match r {
        WindowRequest::Deiconify => match w.minimized {
            Minimized::Yes => Deminiaturize,
            Minimized::No => Nothing(Why::Already),
        },
        WindowRequest::Iconify => match w.minimized {
            Minimized::No => Miniaturize,
            Minimized::Yes => Nothing(Why::Already),
        },
        WindowRequest::Raise => Front,
        WindowRequest::Lower => Back,
        WindowRequest::Refresh => Redraw,
        WindowRequest::FullScreen(how) => match (how, w.full) {
            _ if w.host == Host::Pinned => Nothing(Why::Pinned),
            (FullScreen::Toggle, _)
            | (FullScreen::Enter, Full::No)
            | (FullScreen::Leave, Full::Yes) => ToggleFullScreen,
            _ => Nothing(Why::Already),
        },
        WindowRequest::Move(p) => match (w.full, w.primary_top) {
            (Full::Yes, _) => Nothing(Why::InFullScreen),
            (Full::No, None) => Nothing(Why::NoScreen),
            (Full::No, Some(primary)) => {
                let top = primary - f64::from(p.y.value()) / w.scale;
                Frame(
                    Rect {
                        x: f64::from(p.x.value()) / w.scale,
                        y: top - w.frame.h,
                        ..w.frame
                    },
                    Saved::Keep,
                )
            }
        },
        _ if w.host == Host::Pinned => Nothing(Why::Pinned),
        WindowRequest::ResizePixels { height, width } => match w.full {
            Full::Yes => Nothing(Why::InFullScreen),
            Full::No => content_px(w, width, height).unwrap_or(Nothing(Why::NoScreen)),
        },
        WindowRequest::ResizeCells { rows, cols } => match w.full {
            Full::Yes => Nothing(Why::InFullScreen),
            Full::No => content_cells(w, rows, cols).unwrap_or(Nothing(Why::NoScreen)),
        },
        WindowRequest::Maximize(Maximize::Restore) => match w.saved {
            Some(r) => Frame(r, Saved::Forget),
            None => Nothing(Why::Already),
        },
        WindowRequest::Maximize(how) => match (w.full, w.visible) {
            (Full::Yes, _) => Nothing(Why::InFullScreen),
            (Full::No, None) => Nothing(Why::NoScreen),
            (Full::No, Some(v)) => {
                let f = w.frame;
                let to = match how {
                    Maximize::Vertically => Rect {
                        y: v.y,
                        h: v.h,
                        ..f
                    },
                    Maximize::Horizontally => Rect {
                        x: v.x,
                        w: v.w,
                        ..f
                    },
                    _ => v,
                };
                // xterm's `RequestMaximize` saves the frame once and keeps
                // it while the screen's size is unchanged (it saves again
                // when `restore_width`/`restore_height` differ); baud keeps
                // the first.
                let saved = match w.saved {
                    Some(_) => Saved::Keep,
                    None => Saved::Save(f),
                };
                Frame(to, saved)
            }
        },
    }
}

/// **The calls for one read's requests, and the window they leave**: AppKit
/// performs none of them until the callback returns, so each is measured
/// on the window the calls before it will leave (`after`), not on the one
/// AppKit still has.
pub fn mac_batch(rs: &[WindowRequest], mut w: MacWindow) -> (Vec<MacAction>, MacWindow) {
    let calls = rs
        .iter()
        .map(|&r| {
            let a = mac(r, &w);
            w = after(w, a);
            a
        })
        .collect();
    (calls, w)
}

/// The window `a` leaves `w` as: minimised or not, in full screen or not,
/// or the new frame, its content the frame less the same decoration, the
/// grid the content holds (`grid_in`'s arithmetic) less the status line's
/// rows (`less_status`), and the saved frame.
fn after(mut w: MacWindow, a: MacAction) -> MacWindow {
    match a {
        MacAction::Deminiaturize => w.minimized = Minimized::No,
        MacAction::Miniaturize => w.minimized = Minimized::Yes,
        MacAction::ToggleFullScreen => {
            w.full = match w.full {
                Full::Yes => Full::No,
                Full::No => Full::Yes,
            }
        }
        MacAction::Frame(f, saved) => {
            let deco = decoration(&w);
            w.content = (f.w - deco.0, f.h - deco.1);
            w.frame = f;
            let max = crate::host::MAX_GRID;
            let pad = 2.0 * w.padding * w.scale;
            let line = u16::try_from(w.status.index()).unwrap_or(0);
            // `grid_in`'s arithmetic, its clamp and its conversion.
            let fit = |px: f64, cell: f64, least: u16, most: u16| {
                let n =
                    ((px.max(cell) / cell).floor() as i64).clamp(i64::from(least), i64::from(most));
                u16::try_from(n).unwrap_or(least)
            };
            w.grid = Grid {
                cols: fit(
                    w.content.0 * w.scale - pad - w.gutter,
                    w.cell.0,
                    2,
                    max.ws_col,
                ),
                rows: fit(w.content.1 * w.scale - pad, w.cell.1, 1, max.ws_row)
                    .saturating_sub(line)
                    .max(1),
            };
            match saved {
                Saved::Keep => {}
                Saved::Save(r) => w.saved = Some(r),
                Saved::Forget => w.saved = None,
            }
        }
        MacAction::Front | MacAction::Back | MacAction::Redraw | MacAction::Nothing(_) => {}
    }
    w
}

/// The title bar and anything else the frame has round the content.
fn decoration(w: &MacWindow) -> (f64, f64) {
    (w.frame.w - w.content.0, w.frame.h - w.content.1)
}

/// The frame for a content view `size` points big, its top-left kept, no
/// larger than the screen's visible frame, as xterm's `limitResize` of 1
/// holds a resize to the display's size. **And inside the visible frame
/// top to bottom**: a window that grows down past the visible frame's
/// bottom -- a short one near it taking the status line's row, or a
/// `CSI 8 t` -- moves up until its bottom is on it rather than going
/// under the Dock, and a top above the visible frame's comes down to it,
/// as AppKit's `constrainFrameRect(_:to:)` holds "its top edge" on the
/// screen and its height to the screen's (`[[source:
/// apple-nswindow-constrainframerect]]`). Its "width and horizontal
/// location are unaffected", and so they are here.
fn content_frame(w: &MacWindow, size: (f64, f64)) -> MacAction {
    let deco = decoration(w);
    let (mut fw, mut fh) = (size.0 + deco.0, size.1 + deco.1);
    if let Some(v) = w.visible {
        fw = fw.min(v.w);
        fh = fh.min(v.h);
    }
    let (fw, fh) = (fw.max(deco.0 + 1.0), fh.max(deco.1 + 1.0));
    let y = match w.visible {
        Some(v) => (w.frame.top().min(v.top()) - fh).max(v.y),
        None => w.frame.top() - fh,
    };
    MacAction::Frame(
        Rect {
            x: w.frame.x,
            y,
            w: fw,
            h: fh,
        },
        Saved::Keep,
    )
}

/// `CSI 4 ; height ; width t`: the content view to that many device
/// pixels, as xterm sizes its VT100 widget, the text and its border; an
/// omitted one keeps the content's, 0 is the visible screen's. With no
/// screen, a size is refused: there is nothing to hold it to.
fn content_px(w: &MacWindow, width: SizeAsk, height: SizeAsk) -> Option<MacAction> {
    let deco = decoration(w);
    let side = |ask: SizeAsk, now: f64, screen: Option<f64>| match ask {
        SizeAsk::Keep => Some(now),
        SizeAsk::Screen => screen,
        SizeAsk::Exactly(n) => screen.map(|_| f64::from(n) / w.scale),
    };
    Some(content_frame(
        w,
        (
            side(width, w.content.0, w.visible.map(|v| v.w - deco.0))?,
            side(height, w.content.1, w.visible.map(|v| v.h - deco.1))?,
        ),
    ))
}

/// `CSI 8 ; rows ; cols t` and DECSLPP: the content view a grid that size
/// fills, through the arithmetic `relayout` inverts (`content_width`), an
/// omitted dimension the grid's, 0 the visible screen's, a count past
/// `MAX_GRID` its widest.
fn content_cells(w: &MacWindow, rows: SizeAsk, cols: SizeAsk) -> Option<MacAction> {
    let deco = decoration(w);
    let max = crate::host::MAX_GRID;
    let width = match cols {
        SizeAsk::Screen => w.visible.map(|v| v.w - deco.0)?,
        SizeAsk::Keep => {
            crate::width::content_width(w.grid, w.cell.0, w.scale, w.padding, w.gutter)
        }
        SizeAsk::Exactly(n) => crate::width::content_width(
            Grid {
                cols: n.clamp(2, max.ws_col),
                rows: w.grid.rows,
            },
            w.cell.0,
            w.scale,
            w.padding,
            w.gutter,
        ),
    };
    let height = match rows {
        SizeAsk::Screen => w.visible.map(|v| v.h - deco.1)?,
        SizeAsk::Keep => content_height(w.grid, w),
        SizeAsk::Exactly(n) => content_height(
            Grid {
                cols: w.grid.cols,
                rows: n.clamp(1, max.ws_row),
            },
            w,
        ),
    };
    Some(content_frame(w, (width, height)))
}

/// The content height, in points, of `grid`'s rows and the status line's
/// below them: the cells and the padding top and bottom, and half a pixel
/// more so the floor lands on the rows, as `content_width` does for the
/// columns.
fn content_height(grid: Grid, w: &MacWindow) -> f64 {
    crate::width::content_height(grid, w.status, w.cell.1, w.scale, w.padding)
}

/// **What the status line's coming or going does to the window**:
/// `Term::status_rows` changed, and a row of the window is the line's or
/// stops being it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusChange {
    /// The window grows or shrinks by the row and the grid keeps its rows,
    /// as 070-14 allows, "On a windowed display, the size of the terminal
    /// window may change to include the status line"
    /// (`[[source: dec-std-070]]` p. 14-5), and as xterm's
    /// `resize_status_line` does (`[[source: xterm-reset]]`'s file).
    Resize,
    /// The window keeps its size and the grid gives up its last row to
    /// the line, or takes it back: where the window cannot change size,
    /// in full screen, maximised on Wayland, or with a program's window
    /// operations turned off (`[window] operations`), and on macOS where
    /// the screen has no room for the row ([`mac_status_change`]). A grid
    /// `BAUD_GRID` pins keeps its rows as well, and the frame is a row
    /// taller than the window it is drawn in.
    Relayout,
}

/// [`StatusChange`] for the macOS window: whether it is in full screen,
/// whether a program may resize it, and whether `BAUD_GRID` pins the grid,
/// where the grid keeps its rows and the window its size.
pub fn mac_status(full: Full, ops: WindowOps, host: Host) -> StatusChange {
    match (full, ops, host) {
        (Full::No, WindowOps::On, Host::Window) => StatusChange::Resize,
        _ => StatusChange::Relayout,
    }
}

/// **The frame that holds the grid and the status line's rows below it**,
/// `w.status` the rows the line has now: the content height for both, the
/// width and the top-left kept, as xterm's `resize_status_line` keeps the
/// top with `NorthWestGravity` (`[[source: xterm-reset]]`'s file), and no
/// taller than the visible screen, where the grid gives up what does not
/// fit.
pub fn status_frame(w: &MacWindow) -> MacAction {
    content_frame(w, (w.content.0, content_height(w.grid, w)))
}

/// **[`StatusChange`] for this macOS window now**, `w.status` the rows
/// the line has now and `took` how it took its row when it came. Where
/// [`mac_status`] lets the window change size, the line takes its row from
/// the window only if the screen's visible frame holds the grid and the
/// line, half a device pixel spared; a window already as tall as the
/// screen, or too tall to grow by the whole row, keeps its height and the
/// grid gives the row up. A line going gives its row back as it took it.
pub fn mac_status_change(w: &MacWindow, ops: WindowOps, took: StatusChange) -> StatusChange {
    if mac_status(w.full, ops, w.host) == StatusChange::Relayout {
        return StatusChange::Relayout;
    }
    if w.status == Rows::ZERO {
        return took;
    }
    let wanted = content_height(w.grid, w) + decoration(w).1;
    match w.visible {
        Some(v) if wanted - 0.5 / w.scale > v.h => StatusChange::Relayout,
        _ => StatusChange::Resize,
    }
}

/// **The status line's frame, the first of a pump's calls**, and the window
/// it leaves for [`mac_batch`]: a program's resize in the same read is
/// measured on the window the line grew or shrank, so `CSI 8 ; 30 t` with
/// DECSSDT gives thirty rows and the line, and a move keeps the line's row.
pub fn mac_status_frame(w: MacWindow) -> (MacAction, MacWindow) {
    let a = status_frame(&w);
    (a, after(w, a))
}

/// **The floating Wayland window the status line's change leaves**, `w`
/// the window with `w.status` the rows the line has now and `was` the rows
/// it had: taller or shorter by the line's rows, the grid keeping its own,
/// and the window a program's request in the same read is measured on.
pub fn wayland_status_window(w: WaylandWindow, was: Rows) -> WaylandWindow {
    let rows = |r: Rows| u32::try_from(r.index()).unwrap_or(0);
    let h = (w.size.1 + rows(w.status) * w.cell.1).saturating_sub(rows(was) * w.cell.1);
    WaylandWindow {
        size: (w.size.0, h),
        ..w
    }
}

/// [`StatusChange`] for the Wayland toplevel: a floating window chooses
/// its own size, a maximised or full-screen one obeys the compositor's.
pub fn wayland_status(shown: Shown, ops: WindowOps) -> StatusChange {
    match (shown, ops) {
        (Shown::Floating, WindowOps::On) => StatusChange::Resize,
        _ => StatusChange::Relayout,
    }
}

/// How the compositor last configured the toplevel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shown {
    Floating,
    Maximized,
    FullScreen,
}

/// The Wayland window as the driver has it.
#[derive(Clone, Copy, Debug)]
pub struct WaylandWindow {
    /// The surface, in pixels.
    pub size: (u32, u32),
    /// `configure_bounds`, the monitor less its panels, once sent.
    pub bounds: Option<(u32, u32)>,
    pub shown: Shown,
    /// One cell, in pixels.
    pub cell: (u32, u32),
    pub grid: Grid,
    /// The status line's rows below the grid, as [`MacWindow::status`].
    pub status: Rows,
    /// Whether the exit-status strip, one cell wide, is beside the grid,
    /// which a resize in cells keeps.
    pub marks: baud_render::StatusMarks,
}

/// A toplevel state set or unset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Set {
    On,
    Off,
}

/// What xdg-shell has no request for (`[[source: wp-xdg-shell]]`), each
/// counted under its own reason.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unbuilt {
    /// `CSI 1 t`: `set_minimized` says "There is no way to know if the
    /// surface is currently minimized, nor is there any way to unset
    /// minimization on this surface".
    Unminimize,
    /// `CSI 3 t`: `move` is "an interactive, user-driven move", which "must
    /// be used in response to some sort of user action" and takes a seat and
    /// its serial; a position cannot be asked for.
    Move,
    /// `CSI 5 t` and `CSI 6 t`: xdg-shell has no stacking request.
    Stacking,
    /// `CSI 9 ; 2 t` and `CSI 9 ; 3 t`: `set_maximized` has no axis.
    AxisMaximize,
    /// A resize while maximised, where the configured size "must be
    /// obeyed", or in full screen, where it "is a maximum".
    Bound,
    /// A resize in pixels, or to the screen's size (a 0 in pixels or in
    /// cells), before the compositor has sent its bounds, the only size of
    /// the screen xdg-shell tells a client: none could be held to the
    /// screen, or taken from it.
    NoBounds,
}

/// The xdg-shell request a request makes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaylandAction {
    Minimize,
    SetMaximized(Set),
    SetFullScreen(Set),
    /// A floating window's own size, in pixels.
    Resize(u32, u32),
    Redraw,
    Unbuilt(Unbuilt),
}

/// The xdg-shell request for `r` on window `w`.
pub fn wayland(r: WindowRequest, w: &WaylandWindow) -> WaylandAction {
    use WaylandAction::*;
    let sized = |width: Option<u32>, height: Option<u32>| match (w.shown, width, height) {
        (Shown::Floating, Some(x), Some(y)) => {
            let (x, y) = match w.bounds {
                Some((bw, bh)) => (x.min(bw), y.min(bh)),
                None => (x, y),
            };
            Resize(x.max(1), y.max(1))
        }
        (Shown::Floating, _, _) => Unbuilt(self::Unbuilt::NoBounds),
        _ => Unbuilt(self::Unbuilt::Bound),
    };
    let px = |ask: SizeAsk, now: u32, bound: Option<u32>| match ask {
        SizeAsk::Keep => Some(now),
        SizeAsk::Screen => bound,
        SizeAsk::Exactly(n) => bound.map(|_| u32::from(n)),
    };
    let max = crate::host::MAX_GRID;
    // The status line's rows beside the grid's: a resize in cells gives
    // the grid what it asks and keeps the line's row below it.
    let line = u32::try_from(w.status.index()).unwrap_or(0);
    let cells =
        |ask: SizeAsk, now: u16, cell: u32, most: u16, bound: Option<u32>, extra: u32| match ask {
            SizeAsk::Keep => Some((u32::from(now) + extra) * cell),
            SizeAsk::Screen => bound,
            SizeAsk::Exactly(n) => Some((u32::from(n.clamp(1, most)) + extra) * cell),
        };
    match r {
        WindowRequest::Iconify => Minimize,
        WindowRequest::Deiconify => Unbuilt(self::Unbuilt::Unminimize),
        WindowRequest::Move(_) => Unbuilt(self::Unbuilt::Move),
        WindowRequest::Raise | WindowRequest::Lower => Unbuilt(self::Unbuilt::Stacking),
        WindowRequest::Refresh => Redraw,
        WindowRequest::Maximize(Maximize::Both) => SetMaximized(Set::On),
        WindowRequest::Maximize(Maximize::Restore) => SetMaximized(Set::Off),
        WindowRequest::Maximize(_) => Unbuilt(self::Unbuilt::AxisMaximize),
        WindowRequest::FullScreen(how) => SetFullScreen(match (how, w.shown) {
            (FullScreen::Enter, _) => Set::On,
            (FullScreen::Leave, _) => Set::Off,
            (FullScreen::Toggle, Shown::FullScreen) => Set::Off,
            (FullScreen::Toggle, _) => Set::On,
        }),
        WindowRequest::ResizePixels { height, width } => sized(
            px(width, w.size.0, w.bounds.map(|b| b.0)),
            px(height, w.size.1, w.bounds.map(|b| b.1)),
        ),
        WindowRequest::ResizeCells { rows, cols } => sized(
            cells(
                cols,
                w.grid.cols,
                w.cell.0,
                max.ws_col,
                w.bounds.map(|b| b.0),
                match w.marks {
                    baud_render::StatusMarks::On => 1,
                    baud_render::StatusMarks::Off => 0,
                },
            ),
            cells(
                rows,
                w.grid.rows,
                w.cell.1,
                max.ws_row,
                w.bounds.map(|b| b.1),
                line,
            ),
        ),
    }
}

impl Unbuilt {
    /// Whether xdg-shell lacks the request, which no state of the window
    /// changes, rather than the window's state refusing it now.
    pub fn lacking(self) -> bool {
        match self {
            Unbuilt::Unminimize | Unbuilt::Move | Unbuilt::Stacking | Unbuilt::AxisMaximize => true,
            Unbuilt::Bound | Unbuilt::NoBounds => false,
        }
    }
}

/// **The manipulations the macOS driver carries out, for `OSC 61`**:
/// every form of every one. `mac` makes an AppKit call for each request,
/// and each `Nothing` it answers is the window's state at the time -- a
/// window already as asked, a pinned grid, full screen, no screen -- which
/// a later request finds changed.
pub fn mac_abilities() -> baud_core::WindowAbilities {
    baud_core::WindowAbilities::of(|_| true)
}

/// **Whether the compositor can send `configure_bounds`**, the one size
/// of the screen xdg-shell tells a client: the event is "since 4"
/// (`[[source: wp-xdg-shell]]`), so on an `xdg_wm_base` bound below
/// version 4 it never comes, and before the registry offers the global
/// nothing is known. The Linux window's `ConfigureBounds`, here where
/// the macOS build can test it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoundsEvent {
    /// Version 4 or later: the compositor may send it.
    Offered,
    /// Below version 4, or nothing bound yet: it never comes.
    Never,
}

/// **The manipulations the Linux driver carries out, for `OSC 61`**: each
/// whose every form [`wayland`] answers with a request on a floating
/// window that knows its bounds, the one state that refuses nothing for
/// the state's sake. A form xdg-shell lacks ([`Unbuilt::lacking`]) makes
/// its manipulation one the driver does not do, and so does a form that
/// needs the bounds where the compositor can never send them
/// ([`BoundsEvent::Never`]): then `CSI 4 ; h ; w t`, whose sizes in
/// pixels are held to the bounds, and `CSI 8 ; h ; w t`, whose 0 is "the
/// display's height or width" (`[[source: ctlseqs]]`).
pub fn wayland_abilities(bounds: BoundsEvent) -> baud_core::WindowAbilities {
    let floating = WaylandWindow {
        size: (800, 600),
        bounds: match bounds {
            BoundsEvent::Offered => Some((1920, 1080)),
            BoundsEvent::Never => None,
        },
        shown: Shown::Floating,
        cell: (8, 16),
        grid: Grid {
            cols: 100,
            rows: 37,
        },
        status: Rows::ZERO,
        marks: baud_render::StatusMarks::Off,
    };
    baud_core::WindowAbilities::of(|r| {
        !matches!(wayland(r, &floating), WaylandAction::Unbuilt(u)
            if u.lacking() || u == Unbuilt::NoBounds)
    })
}

/// A reason a request was not done, macOS's or Wayland's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reason {
    Mac(Why),
    Wayland(Unbuilt),
}

/// A driver's count of what it did and did not do, and which reasons it has
/// warned about: each the first time it comes.
pub struct Performed {
    pub done: Tally,
    pub refused: Tally,
    /// The reasons warned about, each once.
    said: Vec<Reason>,
}

impl Performed {
    pub fn new() -> Performed {
        Performed {
            done: Tally::default(),
            refused: Tally::default(),
            said: Vec::new(),
        }
    }

    /// Count a request done.
    pub fn did(&mut self) {
        self.done.bump();
    }

    /// Count a macOS request that did nothing; the warning for its reason,
    /// the first time it comes. A window already as asked is no refusal.
    pub fn mac_nothing(&mut self, why: Why) -> Option<String> {
        let line = match why {
            Why::Already => return None,
            Why::Pinned => "BAUD_GRID pins the grid, so the window keeps its size",
            Why::InFullScreen => "the window is in full screen",
            Why::NoScreen => "the window has no screen to measure against",
        };
        self.refuse(Reason::Mac(why), line)
    }

    /// Count a Wayland request xdg-shell has no request for; the warning
    /// for its reason, the first time it comes.
    pub fn unbuilt(&mut self, why: Unbuilt) -> Option<String> {
        let line = match why {
            Unbuilt::Unminimize => "xdg-shell has no way to undo a minimise",
            Unbuilt::Move => "xdg-shell moves a window only when the user drags it",
            Unbuilt::Stacking => "xdg-shell has no request to raise or lower a window",
            Unbuilt::AxisMaximize => "xdg-shell maximises both ways or not at all",
            Unbuilt::Bound => "the compositor's size binds a maximised or full-screen window",
            Unbuilt::NoBounds => "the compositor has not said how big the screen is",
        };
        self.refuse(Reason::Wayland(why), line)
    }

    fn refuse(&mut self, why: Reason, line: &str) -> Option<String> {
        self.refused.bump();
        if self.said.contains(&why) {
            return None;
        }
        self.said.push(why);
        Some(format!(
            "window operation: a program's request was not done: {line}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{ScreenCoord, ScreenPoint};

    /// A Retina window, 800 x 600 points of content under a 28-point title
    /// bar, its frame's bottom-left at (100, 200) on a 1512 x 982 primary
    /// screen whose visible frame loses the menu bar and the Dock.
    fn window() -> MacWindow {
        MacWindow {
            frame: Rect {
                x: 100.0,
                y: 200.0,
                w: 800.0,
                h: 628.0,
            },
            content: (800.0, 600.0),
            primary_top: Some(982.0),
            visible: Some(Rect {
                x: 0.0,
                y: 70.0,
                w: 1512.0,
                h: 875.0,
            }),
            scale: 2.0,
            cell: (16.0, 34.0),
            padding: 8.0,
            gutter: 0.0,
            grid: Grid { cols: 99, rows: 34 },
            status: Rows::ZERO,
            minimized: Minimized::No,
            full: Full::No,
            saved: None,
            host: Host::Window,
        }
    }

    fn at((x, y): (i32, i32)) -> WindowRequest {
        WindowRequest::Move(ScreenPoint {
            x: ScreenCoord::of(x),
            y: ScreenCoord::of(y),
        })
    }

    fn frame(a: MacAction) -> Rect {
        match a {
            MacAction::Frame(r, _) => r,
            other => panic!("no frame: {other:?}"),
        }
    }

    /// **A move puts the frame's top-left where `CSI 13 t` reports it**:
    /// device pixels from the primary screen's top-left, y down, so
    /// (200, 100) on a 2x screen is 100 points right and 50 down, and a
    /// negative x is a screen to the left. The size stays.
    #[test]
    fn a_move_lands_where_the_report_measures_from() {
        let w = window();
        let f = frame(mac(at((200, 100)), &w));
        assert_eq!((f.x, f.top(), f.w, f.h), (100.0, 932.0, 800.0, 628.0));
        let f = frame(mac(at((-3024, 0)), &w));
        assert_eq!((f.x, f.top()), (-1512.0, 982.0));
        let none = MacWindow {
            primary_top: None,
            ..w
        };
        assert_eq!(mac(at((0, 0)), &none), MacAction::Nothing(Why::NoScreen));
    }

    /// The move is the inverse of the report: the position `window_facts`
    /// reports for a frame, moved to, is that frame.
    #[test]
    fn the_reported_position_moved_to_is_the_same_frame() {
        let w = window();
        let x = (w.frame.x * w.scale) as i32;
        let y = ((982.0 - w.frame.top()) * w.scale) as i32;
        assert_eq!(frame(mac(at((x, y)), &w)), w.frame);
    }

    /// **`CSI 4 ; h ; w t` sizes the content view in device pixels**, height
    /// first, the top-left kept; an omitted dimension keeps the content's,
    /// 0 takes the visible screen's, and the frame never passes the visible
    /// screen (xterm's `limitResize`).
    #[test]
    fn a_pixel_resize_sizes_the_content_view() {
        use SizeAsk::*;
        let w = window();
        let px = |height, width| frame(mac(WindowRequest::ResizePixels { height, width }, &w));
        let f = px(Exactly(1000), Exactly(1400));
        assert_eq!((f.w, f.h, f.x, f.top()), (700.0, 528.0, 100.0, 828.0));
        let f = px(Keep, Exactly(1400));
        assert_eq!((f.w, f.h), (700.0, 628.0), "the height kept");
        let f = px(Screen, Screen);
        assert_eq!((f.w, f.h), (1512.0, 875.0), "the visible screen");
        let f = px(Exactly(60000), Exactly(60000));
        assert_eq!((f.w, f.h), (1512.0, 875.0), "held inside the screen");
    }

    /// **`CSI 8 ; r ; c t` gives the grid `r` by `c`**: the content view
    /// the grid fills, through the arithmetic `relayout` inverts, so the
    /// grid `grid_in` cuts from it is the one asked for. DECSLPP asks the
    /// same with the columns kept.
    #[test]
    fn a_cell_resize_gives_the_grid_asked_for() {
        use SizeAsk::*;
        let w = window();
        let cells = |rows, cols| frame(mac(WindowRequest::ResizeCells { rows, cols }, &w));
        let grid_of = |f: Rect| {
            let content = (
                f.w * w.scale - 2.0 * w.padding * w.scale,
                (f.h - 28.0) * w.scale - 2.0 * w.padding * w.scale,
            );
            (
                (content.1 / w.cell.1).floor(),
                (content.0 / w.cell.0).floor(),
            )
        };
        assert_eq!(grid_of(cells(Exactly(24), Exactly(80))), (24.0, 80.0));
        assert_eq!(grid_of(cells(Exactly(24), Keep)), (24.0, 99.0));
        assert_eq!(grid_of(cells(Keep, Keep)), (34.0, 99.0));
        let f = cells(Exactly(5000), Exactly(5000));
        assert_eq!((f.w, f.h), (1512.0, 875.0), "held inside the screen");
        let f = cells(Screen, Screen);
        assert_eq!((f.w, f.h), (1512.0, 875.0));
        // As tall as the visible frame, it is that frame top to bottom: the
        // top-left kept would put its bottom under the Dock.
        let v = w.visible.expect("a screen");
        assert_eq!((f.x, f.y, f.top()), (w.frame.x, v.y, v.top()));
    }

    /// **A maximise saves the frame once and a restore returns to it**, as
    /// xterm's `RequestMaximize` keeps the frame it restores: both ways to the
    /// visible screen, vertically its height alone, horizontally its width;
    /// a restore with nothing saved does nothing.
    #[test]
    fn a_maximise_saves_the_frame_a_restore_returns_to() {
        let w = window();
        let v = w.visible.unwrap();
        assert_eq!(
            mac(WindowRequest::Maximize(Maximize::Both), &w),
            MacAction::Frame(v, Saved::Save(w.frame))
        );
        let f = frame(mac(WindowRequest::Maximize(Maximize::Vertically), &w));
        assert_eq!((f.x, f.w, f.y, f.h), (100.0, 800.0, v.y, v.h));
        let f = frame(mac(WindowRequest::Maximize(Maximize::Horizontally), &w));
        assert_eq!((f.x, f.w, f.y, f.h), (v.x, v.w, 200.0, 628.0));
        let saved = MacWindow {
            saved: Some(w.frame),
            frame: v,
            ..w
        };
        assert_eq!(
            mac(WindowRequest::Maximize(Maximize::Both), &saved),
            MacAction::Frame(v, Saved::Keep),
            "a second maximise keeps the first's frame"
        );
        assert_eq!(
            mac(WindowRequest::Maximize(Maximize::Restore), &saved),
            MacAction::Frame(w.frame, Saved::Forget)
        );
        assert_eq!(
            mac(WindowRequest::Maximize(Maximize::Restore), &w),
            MacAction::Nothing(Why::Already)
        );
    }

    /// The calls that only toggle are made only when they change something:
    /// a de-iconify of an open window, an iconify of a minimised one, a full
    /// screen entered twice do nothing; `CSI 10 ; 2 t` always toggles.
    #[test]
    fn a_toggle_is_called_only_when_it_changes_the_window() {
        let w = window();
        let min = MacWindow {
            minimized: Minimized::Yes,
            ..w
        };
        let full = MacWindow {
            full: Full::Yes,
            ..w
        };
        let fs = WindowRequest::FullScreen;
        for (r, on, want) in [
            (
                WindowRequest::Deiconify,
                &w,
                MacAction::Nothing(Why::Already),
            ),
            (WindowRequest::Deiconify, &min, MacAction::Deminiaturize),
            (WindowRequest::Iconify, &w, MacAction::Miniaturize),
            (
                WindowRequest::Iconify,
                &min,
                MacAction::Nothing(Why::Already),
            ),
            (fs(FullScreen::Enter), &w, MacAction::ToggleFullScreen),
            (
                fs(FullScreen::Enter),
                &full,
                MacAction::Nothing(Why::Already),
            ),
            (fs(FullScreen::Leave), &w, MacAction::Nothing(Why::Already)),
            (fs(FullScreen::Leave), &full, MacAction::ToggleFullScreen),
            (fs(FullScreen::Toggle), &w, MacAction::ToggleFullScreen),
            (fs(FullScreen::Toggle), &full, MacAction::ToggleFullScreen),
            (WindowRequest::Raise, &w, MacAction::Front),
            (WindowRequest::Lower, &w, MacAction::Back),
            (WindowRequest::Refresh, &w, MacAction::Redraw),
        ] {
            assert_eq!(mac(r, on), want, "{r:?}");
        }
    }

    /// In full screen the window has the screen, so a move, a resize and a
    /// maximise do nothing until it leaves; a pinned grid keeps the window's
    /// size, so a resize, a maximise and full screen do nothing, and a move
    /// still moves.
    #[test]
    fn full_screen_and_a_pinned_grid_keep_the_frame() {
        use SizeAsk::*;
        let w = window();
        let full = MacWindow {
            full: Full::Yes,
            ..w
        };
        let pinned = MacWindow {
            host: Host::Pinned,
            ..w
        };
        let sizing = [
            WindowRequest::ResizePixels {
                height: Exactly(100),
                width: Exactly(100),
            },
            WindowRequest::ResizeCells {
                rows: Exactly(24),
                cols: Keep,
            },
            WindowRequest::Maximize(Maximize::Both),
        ];
        for r in sizing.into_iter().chain([at((0, 0))]) {
            assert_eq!(
                mac(r, &full),
                MacAction::Nothing(Why::InFullScreen),
                "{r:?}"
            );
        }
        for r in sizing
            .into_iter()
            .chain([WindowRequest::FullScreen(FullScreen::Toggle)])
        {
            assert_eq!(mac(r, &pinned), MacAction::Nothing(Why::Pinned), "{r:?}");
        }
        assert!(matches!(mac(at((0, 0)), &pinned), MacAction::Frame(..)));
    }

    /// **With no screen to measure against, a pixel resize is refused**, as
    /// a resize to the screen's size is: xterm's `limitResize` holds every
    /// resize to the display, and without one there is nothing to hold it
    /// to. A resize in cells is held to the largest grid either way.
    #[test]
    fn with_no_screen_a_pixel_resize_is_refused() {
        use SizeAsk::*;
        let w = MacWindow {
            visible: None,
            ..window()
        };
        for (height, width) in [
            (Exactly(60000), Exactly(60000)),
            (Keep, Exactly(60000)),
            (Exactly(1000), Keep),
        ] {
            assert_eq!(
                mac(WindowRequest::ResizePixels { height, width }, &w),
                MacAction::Nothing(Why::NoScreen),
                "{height:?} by {width:?}"
            );
        }
        let f = frame(mac(
            WindowRequest::ResizeCells {
                rows: Exactly(65535),
                cols: Exactly(65535),
            },
            &w,
        ));
        let max = crate::host::MAX_GRID;
        let most = Grid {
            cols: max.ws_col,
            rows: max.ws_row,
        };
        let deco = (w.frame.w - w.content.0, w.frame.h - w.content.1);
        assert_eq!(
            (f.w, f.h),
            (
                crate::width::content_width(most, w.cell.0, w.scale, w.padding, w.gutter) + deco.0,
                content_height(most, &w) + deco.1
            ),
            "the largest grid"
        );
    }

    /// **The requests one read brings are each measured on the window the
    /// ones before them leave**, since AppKit performs none until the
    /// callback returns: a move then a resize keeps the moved top-left, a
    /// resize then a grid with its columns kept keeps the new width, an
    /// iconify then a de-iconify does both, a maximise then a restore
    /// returns to the frame the maximise saved, and a move after entering
    /// full screen waits.
    #[test]
    fn a_batch_is_measured_on_the_window_each_request_leaves() {
        use SizeAsk::*;
        let w = window();
        let (calls, _) = mac_batch(
            &[
                at((200, 100)),
                WindowRequest::ResizePixels {
                    height: Exactly(1000),
                    width: Exactly(1400),
                },
            ],
            w,
        );
        let f = frame(calls[1]);
        assert_eq!((f.x, f.top(), f.w, f.h), (100.0, 932.0, 700.0, 528.0));
        let (calls, _) = mac_batch(
            &[
                WindowRequest::ResizePixels {
                    height: Keep,
                    width: Exactly(1400),
                },
                WindowRequest::ResizeCells {
                    rows: Exactly(24),
                    cols: Keep,
                },
            ],
            w,
        );
        let f = frame(calls[1]);
        let cols = ((f.w * w.scale - 2.0 * w.padding * w.scale) / w.cell.0).floor();
        assert_eq!(cols, 85.0, "the columns the resize left");
        let (calls, _) = mac_batch(&[WindowRequest::Iconify, WindowRequest::Deiconify], w);
        assert_eq!(calls, [MacAction::Miniaturize, MacAction::Deminiaturize]);
        let v = w.visible.unwrap();
        let (calls, left) = mac_batch(
            &[
                WindowRequest::Maximize(Maximize::Both),
                WindowRequest::Maximize(Maximize::Restore),
            ],
            w,
        );
        assert_eq!(
            calls,
            [
                MacAction::Frame(v, Saved::Save(w.frame)),
                MacAction::Frame(w.frame, Saved::Forget)
            ]
        );
        assert_eq!((left.frame, left.saved), (w.frame, None));
        let (calls, left) = mac_batch(
            &[WindowRequest::FullScreen(FullScreen::Enter), at((0, 0))],
            w,
        );
        assert_eq!(
            calls,
            [
                MacAction::ToggleFullScreen,
                MacAction::Nothing(Why::InFullScreen)
            ]
        );
        assert_eq!(left.full, Full::Yes);
    }

    fn wayland_window() -> WaylandWindow {
        WaylandWindow {
            size: (1280, 720),
            bounds: Some((2560, 1400)),
            shown: Shown::Floating,
            cell: (16, 32),
            grid: Grid { cols: 80, rows: 22 },
            status: Rows::ZERO,
            marks: baud_render::StatusMarks::Off,
        }
    }

    /// **What xdg-shell has, Linux does; the rest is counted with why**:
    /// minimise, maximise both ways and restore, full screen by the state
    /// the configure carried, and a floating window's own size, cells times
    /// the cell, held inside the compositor's bounds.
    #[test]
    fn wayland_does_what_xdg_shell_has() {
        use SizeAsk::*;
        use WaylandAction::*;
        let w = wayland_window();
        let full = WaylandWindow {
            shown: Shown::FullScreen,
            ..w
        };
        let max = WaylandWindow {
            shown: Shown::Maximized,
            ..w
        };
        let no_bounds = WaylandWindow { bounds: None, ..w };
        let marked = WaylandWindow {
            marks: baud_render::StatusMarks::On,
            ..w
        };
        let px = |height, width| WindowRequest::ResizePixels { height, width };
        let cells = |rows, cols| WindowRequest::ResizeCells { rows, cols };
        for (r, on, want) in [
            (WindowRequest::Iconify, &w, Minimize),
            (
                WindowRequest::Deiconify,
                &w,
                Unbuilt(super::Unbuilt::Unminimize),
            ),
            (at((0, 0)), &w, Unbuilt(super::Unbuilt::Move)),
            (WindowRequest::Raise, &w, Unbuilt(super::Unbuilt::Stacking)),
            (WindowRequest::Lower, &w, Unbuilt(super::Unbuilt::Stacking)),
            (WindowRequest::Refresh, &w, Redraw),
            (
                WindowRequest::Maximize(Maximize::Both),
                &w,
                SetMaximized(Set::On),
            ),
            (
                WindowRequest::Maximize(Maximize::Restore),
                &max,
                SetMaximized(Set::Off),
            ),
            (
                WindowRequest::Maximize(Maximize::Vertically),
                &w,
                Unbuilt(super::Unbuilt::AxisMaximize),
            ),
            (
                WindowRequest::FullScreen(FullScreen::Toggle),
                &w,
                SetFullScreen(Set::On),
            ),
            (
                WindowRequest::FullScreen(FullScreen::Toggle),
                &full,
                SetFullScreen(Set::Off),
            ),
            (
                WindowRequest::FullScreen(FullScreen::Enter),
                &full,
                SetFullScreen(Set::On),
            ),
            (
                WindowRequest::FullScreen(FullScreen::Leave),
                &w,
                SetFullScreen(Set::Off),
            ),
            (px(Exactly(600), Exactly(800)), &w, Resize(800, 600)),
            (px(Keep, Exactly(800)), &w, Resize(800, 720)),
            (px(Screen, Screen), &w, Resize(2560, 1400)),
            (px(Exactly(9000), Exactly(9000)), &w, Resize(2560, 1400)),
            (
                px(Screen, Keep),
                &no_bounds,
                Unbuilt(super::Unbuilt::NoBounds),
            ),
            (
                px(Exactly(65535), Exactly(65535)),
                &no_bounds,
                Unbuilt(super::Unbuilt::NoBounds),
            ),
            (
                px(Keep, Exactly(800)),
                &no_bounds,
                Unbuilt(super::Unbuilt::NoBounds),
            ),
            (
                cells(Exactly(65535), Exactly(65535)),
                &no_bounds,
                Resize(6400, 4800),
            ),
            (cells(Exactly(24), Exactly(80)), &w, Resize(1280, 768)),
            // The exit-status strip is a cell beside the grid's 80.
            (cells(Exactly(24), Exactly(80)), &marked, Resize(1296, 768)),
            (cells(Exactly(48), Keep), &w, Resize(1280, 1400)),
            (cells(Keep, Keep), &w, Resize(1280, 704)),
            (
                cells(Exactly(24), Keep),
                &max,
                Unbuilt(super::Unbuilt::Bound),
            ),
            (
                cells(Exactly(24), Keep),
                &full,
                Unbuilt(super::Unbuilt::Bound),
            ),
        ] {
            assert_eq!(wayland(r, on), want, "{r:?} on {:?}", on.shown);
        }
    }

    /// **The status line takes a row of the window, not of the grid**:
    /// a floating window grows by it, where a program may resize the window;
    /// in full screen, maximised on Wayland or with the operations off the
    /// grid gives the row up instead, and a pinned grid keeps its rows.
    #[test]
    fn the_status_line_grows_the_window_where_it_can_change_size() {
        use StatusChange::*;
        assert_eq!(mac_status(Full::No, WindowOps::On, Host::Window), Resize);
        assert_eq!(mac_status(Full::Yes, WindowOps::On, Host::Window), Relayout);
        assert_eq!(mac_status(Full::No, WindowOps::Off, Host::Window), Relayout);
        assert_eq!(mac_status(Full::No, WindowOps::On, Host::Pinned), Relayout);
        assert_eq!(wayland_status(Shown::Floating, WindowOps::On), Resize);
        assert_eq!(wayland_status(Shown::Maximized, WindowOps::On), Relayout);
        assert_eq!(wayland_status(Shown::FullScreen, WindowOps::On), Relayout);
        assert_eq!(wayland_status(Shown::Floating, WindowOps::Off), Relayout);
    }

    /// **The status line's row grows the frame downward**: one cell taller,
    /// the top-left and the width kept, and back to the frame it had when
    /// the line goes.
    #[test]
    fn the_status_frame_grows_down_by_a_row() {
        let w = window();
        let lined = MacWindow {
            status: Rows::of_count(1),
            ..w
        };
        let grown = frame(status_frame(&lined));
        assert_eq!(grown.top(), w.frame.top());
        assert_eq!((grown.x, grown.w), (w.frame.x, w.frame.w));
        let bare = frame(status_frame(&w));
        assert_eq!(grown.h - bare.h, w.cell.1 / w.scale);
    }

    /// **A window near the visible frame's bottom stays above it**: the
    /// status line's row, which grows the frame down, and a `CSI 8 t` that
    /// makes it taller move a short window up until its bottom is the
    /// visible frame's, its height and x kept, where the frame went under
    /// the Dock; one whose top is above the visible frame's comes down to
    /// it. The control: a window with room below keeps its top.
    #[test]
    fn a_frame_that_grows_stays_inside_the_visible_frame() {
        let low = MacWindow {
            frame: Rect {
                x: 100.0,
                y: 80.0,
                w: 800.0,
                h: 228.0,
            },
            content: (800.0, 200.0),
            grid: Grid { cols: 99, rows: 11 },
            ..window()
        };
        let v = low.visible.expect("a screen");
        let lined = MacWindow {
            status: Rows::of_count(1),
            ..low
        };
        let grown = frame(status_frame(&lined));
        assert_eq!(grown.y, v.y, "{grown:?}");
        assert_eq!((grown.x, grown.w), (low.frame.x, low.frame.w));
        assert_eq!(
            grown.h,
            frame(status_frame(&low)).h + low.cell.1 / low.scale
        );
        let rows = WindowRequest::ResizeCells {
            rows: SizeAsk::Exactly(20),
            cols: SizeAsk::Keep,
        };
        let taller = frame(mac(rows, &low));
        assert_eq!(taller.y, v.y, "{taller:?}");
        assert!(taller.h > low.frame.h);
        let high = MacWindow {
            frame: Rect {
                y: v.top() - 100.0,
                ..low.frame
            },
            ..low
        };
        let f = frame(mac(rows, &high));
        assert_eq!(f.top(), v.top(), "{f:?}");
        // The control.
        let roomy = frame(status_frame(&MacWindow {
            status: Rows::of_count(1),
            ..window()
        }));
        assert_eq!(roomy.top(), window().frame.top());
    }

    /// **A window as tall as the screen gives the grid's row up**: zoomed,
    /// tiled or dragged to the visible frame's height, or short of it by
    /// less than the row, the window keeps its frame and the grid gives the
    /// row to the line, and takes it back when the line goes; a window with
    /// the room grows, and shrinks again.
    #[test]
    fn a_window_as_tall_as_the_screen_gives_the_grid_row_up() {
        use StatusChange::*;
        let one = Rows::of_count(1);
        let tall = MacWindow {
            frame: Rect {
                x: 100.0,
                y: 70.0,
                w: 800.0,
                h: 875.0,
            },
            content: (800.0, 847.0),
            grid: Grid { cols: 99, rows: 48 },
            status: one,
            ..window()
        };
        let on = WindowOps::On;
        assert_eq!(mac_status_change(&tall, on, Resize), Relayout);
        let bare = MacWindow {
            status: Rows::ZERO,
            ..tall
        };
        assert_eq!(mac_status_change(&bare, on, Relayout), Relayout);
        let short = MacWindow {
            frame: Rect {
                y: 77.0,
                h: 868.0,
                ..tall.frame
            },
            content: (800.0, 840.0),
            ..tall
        };
        assert_eq!(mac_status_change(&short, on, Resize), Relayout);
        let roomy = MacWindow {
            status: one,
            ..window()
        };
        assert_eq!(mac_status_change(&roomy, on, Relayout), Resize);
        assert_eq!(mac_status_change(&window(), on, Resize), Resize);
    }

    /// **The status line's frame comes first in a pump**, so a program's
    /// request in the same read is measured on the window it leaves:
    /// `CSI 8 ; 30 t` gives thirty rows and the line, an omitted height the
    /// grid's rows and the line's, and a move keeps the grown height.
    #[test]
    fn the_status_lines_frame_comes_before_a_pumps_requests() {
        use SizeAsk::*;
        let lined = MacWindow {
            status: Rows::of_count(1),
            ..window()
        };
        let (grown, left) = mac_status_frame(lined);
        let grown = frame(grown);
        let rows = |n: u16| {
            crate::width::content_height(
                Grid { cols: 99, rows: n },
                lined.status,
                lined.cell.1,
                lined.scale,
                lined.padding,
            ) + (lined.frame.h - lined.content.1)
        };
        assert_eq!(grown.h, rows(34));
        let thirty = WindowRequest::ResizeCells {
            rows: Exactly(30),
            cols: Keep,
        };
        let keep = WindowRequest::ResizeCells {
            rows: Keep,
            cols: Keep,
        };
        for (asked, want) in [
            (thirty, rows(30)),
            (keep, rows(34)),
            (at((200, 100)), rows(34)),
        ] {
            let (calls, _) = mac_batch(&[asked], left);
            assert_eq!(frame(calls[0]).h, want, "{asked:?}");
        }
    }

    /// **A floating Wayland window grows by the line's row and gives it
    /// back**, and a request in the same read is measured on the grown
    /// window: thirty rows and the line.
    #[test]
    fn a_wayland_window_grows_by_the_line_first() {
        use SizeAsk::*;
        let one = Rows::of_count(1);
        let lined = WaylandWindow {
            status: one,
            ..wayland_window()
        };
        let ww = wayland_status_window(lined, Rows::ZERO);
        assert_eq!(ww.size, (1280, 752));
        let bare = WaylandWindow {
            status: Rows::ZERO,
            ..ww
        };
        assert_eq!(wayland_status_window(bare, one).size, (1280, 720));
        let thirty = WindowRequest::ResizeCells {
            rows: Exactly(30),
            cols: Keep,
        };
        assert_eq!(wayland(thirty, &ww), WaylandAction::Resize(1280, 31 * 32));
    }

    /// **Both drivers act on the status line before a read's window
    /// operations, and the macOS pointer draws the indicator's frame**:
    /// AppKit- and Wayland-bound, so the source is held. Linux resizes for
    /// the line before the requests, as the macOS pump queues the line's
    /// frame before them (`the_scrollbar_mode_reaches_the_window`); every
    /// locator half the macOS mouse handler tells is acted on, its report
    /// sent and the indicator's frame drawn (`crate::gesture::locator_redraws`),
    /// as the seat's is (`a_move_redraws_the_indicator_line`).
    #[test]
    fn the_drivers_act_on_the_line_as_the_seams_say() {
        let linux = include_str!("linux.rs");
        assert_eq!(linux.matches("self.status_line();").count(), 1);
        assert!(
            linux.contains(concat!(
                "        self.status_line();\n",
                "        for r in asked {\n",
                "            self.window_request(r);"
            )),
            "the line's resize before the requests"
        );
        // The two halves the mouse handler tells, each acted on: the
        // seam itself is `the_driver_tells_the_locator_each_half_once`.
        let app = include_str!("app.rs");
        let told = concat!(
            "        let owed = tell_locator(&mut self.app.term, pointer, half, at, self.buttons);\n",
            "        self.locator_owed(owed);\n"
        );
        assert_eq!(app.matches(told).count(), 2, "two locator halves");
        let owed = concat!(
            "        if owed.reply == Reply::Queued {\n",
            "            self.app.flush_reply();\n",
            "        }\n",
            "        if owed.frame == LineFrame::Owed {\n",
            "            self.app.pending = Pending::Dirty;\n",
            "        }\n"
        );
        assert_eq!(
            app.matches(owed).count(),
            1,
            "the report sent, the frame drawn"
        );
    }

    /// **A resize in cells gives the grid the rows asked for and keeps the
    /// status line's below them**: `CSI 8 ; 24 ; 80 t` with a line shown is
    /// one cell taller than without, on both drivers, and an omitted height
    /// keeps the grid's rows and the line's.
    #[test]
    fn a_cell_resize_keeps_the_status_lines_row() {
        use SizeAsk::*;
        let w = window();
        let lined = MacWindow {
            status: Rows::of_count(1),
            ..w
        };
        let ask = WindowRequest::ResizeCells {
            rows: Exactly(24),
            cols: Keep,
        };
        let (bare, with) = (frame(mac(ask, &w)), frame(mac(ask, &lined)));
        assert_eq!(with.h - bare.h, w.cell.1 / w.scale);
        assert_eq!(with.top(), bare.top(), "the top-left kept");
        let ww = wayland_window();
        let lined = WaylandWindow {
            status: Rows::of_count(1),
            ..ww
        };
        let keep = WindowRequest::ResizeCells {
            rows: Keep,
            cols: Keep,
        };
        assert_eq!(wayland(keep, &ww), WaylandAction::Resize(1280, 22 * 32));
        assert_eq!(wayland(keep, &lined), WaylandAction::Resize(1280, 23 * 32));
    }

    /// Each reason warns once and every refusal counts; a window already as
    /// asked is neither.
    #[test]
    fn each_reason_warns_once_and_every_refusal_counts() {
        let mut p = Performed::new();
        assert!(p.mac_nothing(Why::Already).is_none());
        assert_eq!(p.refused.value(), 0);
        assert!(p.mac_nothing(Why::Pinned).is_some());
        assert!(p.mac_nothing(Why::Pinned).is_none());
        assert!(p.unbuilt(Unbuilt::Move).is_some());
        assert!(p.unbuilt(Unbuilt::Stacking).is_some());
        assert!(p.unbuilt(Unbuilt::Move).is_none());
        assert_eq!(p.refused.value(), 5);
        p.did();
        assert_eq!(p.done.value(), 1);
    }

    /// **What each driver tells `OSC 61` it does is what its policy does**:
    /// macOS every manipulation, and Linux all but the five whose forms
    /// xdg-shell lacks -- de-iconify, move, raise, lower, and maximise on
    /// one axis, which takes `MaximizeWin` with it. Every form of every
    /// manipulation is walked through `wayland` on each state the window can
    /// be in: one answered with a lacking request is never claimed, and a
    /// claimed one is refused only for the state's sake.
    #[test]
    fn the_abilities_are_what_each_driver_does() {
        use baud_core::WindowManipulation as M;
        let mac = mac_abilities();
        assert!(M::ALL.iter().all(|&m| mac.has(m)), "macOS does them all");
        let linux = wayland_abilities(BoundsEvent::Offered);
        let not = [
            M::RestoreWin,
            M::SetWinPosition,
            M::RaiseWin,
            M::LowerWin,
            M::MaximizeWin,
        ];
        for m in M::ALL {
            assert_eq!(linux.has(m), !not.contains(&m), "{m:?}");
        }
        for shown in [Shown::Floating, Shown::Maximized, Shown::FullScreen] {
            for bounds in [None, Some((1920, 1080))] {
                let w = WaylandWindow {
                    size: (800, 600),
                    bounds,
                    shown,
                    cell: (8, 16),
                    grid: Grid {
                        cols: 100,
                        rows: 37,
                    },
                    status: Rows::ZERO,
                    marks: baud_render::StatusMarks::Off,
                };
                for m in M::ALL {
                    let lacks = !m.done_by(
                        &|r| !matches!(wayland(r, &w), WaylandAction::Unbuilt(u) if u.lacking()),
                    );
                    assert_eq!(lacks, !linux.has(m), "{m:?} {shown:?} {bounds:?}");
                }
            }
        }
    }

    /// **`OSC 61` answers what the bound `xdg_wm_base` allows**:
    /// `configure_bounds` is "since 4" (`[[source: wp-xdg-shell]]`), so
    /// below version 4, or before the global is bound, no bounds ever come,
    /// so a resize in pixels is never done and neither is a size of 0, "the
    /// display's height or width" (`[[source: ctlseqs]]`), in pixels or in
    /// cells: `SetWinSizePixels` and `SetWinSizeChars` are listed, and from
    /// 4 on both are claimed. `SetWinLines`, rows by number only, needs no
    /// bounds and is claimed on both. Every manipulation claimed below 4 is
    /// done, every form, by [`wayland`] on a floating window with no bounds.
    #[test]
    fn osc_61_answers_what_the_bound_xdg_wm_base_allows() {
        use baud_core::WindowManipulation as M;
        let below = wayland_abilities(BoundsEvent::Never);
        let from = wayland_abilities(BoundsEvent::Offered);
        for m in [M::SetWinSizePixels, M::SetWinSizeChars] {
            assert!(!below.has(m), "{m:?} below 4");
            assert!(from.has(m), "{m:?} from 4");
        }
        assert!(
            below.has(M::SetWinLines) && from.has(M::SetWinLines),
            "SetWinLines on both"
        );
        let floating = WaylandWindow {
            size: (800, 600),
            bounds: None,
            shown: Shown::Floating,
            cell: (8, 16),
            grid: Grid {
                cols: 100,
                rows: 37,
            },
            status: Rows::ZERO,
            marks: baud_render::StatusMarks::Off,
        };
        for m in M::ALL.iter().copied().filter(|&m| below.has(m)) {
            let done = m.done_by(&|r| !matches!(wayland(r, &floating), WaylandAction::Unbuilt(_)));
            assert!(done, "{m:?} claimed below 4 and not done there");
        }
    }
}
