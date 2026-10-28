//! What an embedder knows about its window and screen, for XTWINOPS's
//! reports, and what a program asks of the window, XTWINOPS 1-10 and
//! DECSLPP (`[[source: ctlseqs]]`, "Window manipulation (XTWINOPS)").
//!
//! The core has no window. Each fact here is one the embedder hands over
//! (`Term::set_window`) or leaves `None`, and a report whose fact is `None`
//! is not answered: silence is the answer an application already handles
//! for a terminal that does not implement the report, and a made-up number
//! is not. Each request is one the core queues for the embedder
//! ([`WindowRequest`]), which moves, sizes or stacks its window.

use crate::{ImagePx, Param, ParamIdx, Params, WindowOp};

/// Whether the window is iconified, `CSI 11 t`'s answer: "If the xterm
/// window is non-iconified, it returns CSI 1 t . If the xterm window is
/// iconified, it returns CSI 2 t".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowState {
    Open,
    Iconified,
}

impl WindowState {
    pub(crate) fn code(self) -> u16 {
        match self {
            WindowState::Open => 1,
            WindowState::Iconified => 2,
        }
    }
}

/// One coordinate on the screen, in device pixels from the left or the top
/// of the primary screen. **Signed**: a window on a screen left of or above
/// the primary one is at a negative position.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenCoord(i32);

impl ScreenCoord {
    pub const ZERO: ScreenCoord = ScreenCoord(0);

    pub fn of(px: i32) -> ScreenCoord {
        ScreenCoord(px)
    }

    /// The coordinate, in device pixels.
    pub fn value(self) -> i32 {
        self.0
    }

    /// The coordinate as XTWINOPS writes it: "X Toolkit positions can be
    /// negative, but the reported values are unsigned, in the range
    /// 0-65535. Negative values correspond to 32768-65535"
    /// (`[[source: ctlseqs]]`) -- the low sixteen bits of the two's
    /// complement, so -1 is 65535.
    pub fn wire(self) -> u16 {
        u16::try_from(self.0.rem_euclid(1 << 16)).unwrap_or(u16::MAX)
    }

    /// A coordinate a program sent, read as [`ScreenCoord::wire`] writes one:
    /// 32768-65535 are -32768 to -1. An ECMA-48 parameter has no minus sign
    /// ("each of which represents a number in decimal notation", 5.4.2), and
    /// xterm's move reads its two through X's signed sixteen-bit `Position`
    /// (`window_ops`, `[[source: xterm-reset]]`'s file), so the position
    /// `CSI 13 t` reports, sent back, is where the window goes.
    pub fn from_wire(v: u16) -> ScreenCoord {
        ScreenCoord(i32::from(i16::from_ne_bytes(v.to_ne_bytes())))
    }
}

/// A point on the screen: a window's or a text area's top-left corner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenPoint {
    pub x: ScreenCoord,
    pub y: ScreenCoord,
}

/// A size in device pixels, the unit `CSI 14 t` and `CSI 16 t` answer in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PixelSize {
    pub width: ImagePx,
    pub height: ImagePx,
}

/// The window and screen facts an embedder has, each `None` until it says.
///
/// * `state`: open or iconified (`CSI 11 t`).
/// * `position`: the window's top-left, its frame included (`CSI 13 t`).
/// * `text_area`: the text area's first pixel on the screen, the origin a
///   mouse report in pixels counts from (`CSI 13 ; 2 t`).
/// * `window`: the window's size, frame and decoration included
///   (`CSI 14 ; 2 t`).
/// * `screen`: the size of the screen the window is on (`CSI 15 t`), and,
///   divided by the cell, `CSI 19 t`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WindowFacts {
    pub state: Option<WindowState>,
    pub position: Option<ScreenPoint>,
    pub text_area: Option<ScreenPoint>,
    pub window: Option<PixelSize>,
    pub screen: Option<PixelSize>,
}

impl WindowFacts {
    /// Nothing known: every window report is silent.
    pub const UNKNOWN: WindowFacts = WindowFacts {
        state: None,
        position: None,
        text_area: None,
        window: None,
        screen: None,
    };
}

/// Whether the window operations, XTWINOPS 1-10 and DECSLPP, reach the
/// embedder. `On` by default, and `Off` refuses every one, counted and
/// nothing queued, as xterm does with them among its disallowed window
/// operations (`AllowWindowOps`, `[[source: xterm-reset]]`'s file). A
/// program on the other end of the line, a remote one included, can move,
/// resize, hide or raise the window, which is why a setting turns them
/// off. The reports and the title stack are not window operations here
/// and answer either way.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum WindowOps {
    #[default]
    On,
    Off,
}

/// One of the window manipulations among xterm's `allowWindowOps`
/// subcategories (`tblWindowOps`, `[[source: xterm-reset]]`'s file;
/// `ewRestoreWin` to `ewFullscreenWin` and `ewSetWinLines`,
/// `[[source: xterm-ptyx-h]]`): a program's request the embedder carries
/// out, so that `OSC 61 ; allowWindowOps` lists the ones it does not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowManipulation {
    /// `CSI 1 t`.
    RestoreWin,
    /// `CSI 2 t`.
    MinimizeWin,
    /// `CSI 3 ; x ; y t`.
    SetWinPosition,
    /// `CSI 4 ; height ; width t`.
    SetWinSizePixels,
    /// `CSI 5 t`.
    RaiseWin,
    /// `CSI 6 t`.
    LowerWin,
    /// `CSI 7 t`.
    RefreshWin,
    /// `CSI 8 ; rows ; cols t`.
    SetWinSizeChars,
    /// `CSI 9 ; Ps t`, all four forms.
    MaximizeWin,
    /// `CSI 10 ; Ps t`, all three forms.
    FullscreenWin,
    /// DECSLPP, `CSI Ps t` with `Ps` 24 and up, "this item uses all
    /// remaining numbers in the sequence".
    SetWinLines,
}

impl WindowManipulation {
    /// In `tblWindowOps`' order.
    pub const ALL: [WindowManipulation; 11] = [
        WindowManipulation::RestoreWin,
        WindowManipulation::MinimizeWin,
        WindowManipulation::SetWinPosition,
        WindowManipulation::SetWinSizePixels,
        WindowManipulation::RaiseWin,
        WindowManipulation::LowerWin,
        WindowManipulation::RefreshWin,
        WindowManipulation::SetWinSizeChars,
        WindowManipulation::MaximizeWin,
        WindowManipulation::FullscreenWin,
        WindowManipulation::SetWinLines,
    ];

    /// The subcategory's name, as `OSC 61` spells it.
    pub fn name(self) -> &'static [u8] {
        match self {
            WindowManipulation::RestoreWin => b"RestoreWin",
            WindowManipulation::MinimizeWin => b"MinimizeWin",
            WindowManipulation::SetWinPosition => b"SetWinPosition",
            WindowManipulation::SetWinSizePixels => b"SetWinSizePixels",
            WindowManipulation::RaiseWin => b"RaiseWin",
            WindowManipulation::LowerWin => b"LowerWin",
            WindowManipulation::RefreshWin => b"RefreshWin",
            WindowManipulation::SetWinSizeChars => b"SetWinSizeChars",
            WindowManipulation::MaximizeWin => b"MaximizeWin",
            WindowManipulation::FullscreenWin => b"FullscreenWin",
            WindowManipulation::SetWinLines => b"SetWinLines",
        }
    }

    /// The manipulation `name` is, or `None` for another subcategory.
    pub fn of_name(name: &[u8]) -> Option<WindowManipulation> {
        WindowManipulation::ALL
            .into_iter()
            .find(|m| m.name() == name)
    }

    /// Whether `does` carries out every form of it: a subcategory done in
    /// part is not done, as `SetColor` is refused for the colours it
    /// refuses. Each form is given with typical values, since what an
    /// embedder cannot do is a kind of request and not a size; a resize's
    /// forms are a size given and a size of 0, "the display's height or
    /// width" (`[[source: ctlseqs]]`), which an embedder that does not know
    /// the display's size cannot do.
    pub fn done_by(self, does: &impl Fn(WindowRequest) -> bool) -> bool {
        let size = SizeAsk::Exactly(24);
        let screen = SizeAsk::Screen;
        let forms: &[WindowRequest] = match self {
            WindowManipulation::RestoreWin => &[WindowRequest::Deiconify],
            WindowManipulation::MinimizeWin => &[WindowRequest::Iconify],
            WindowManipulation::SetWinPosition => &[WindowRequest::Move(ScreenPoint {
                x: ScreenCoord(0),
                y: ScreenCoord(0),
            })],
            WindowManipulation::SetWinSizePixels => &[
                WindowRequest::ResizePixels {
                    height: size,
                    width: size,
                },
                WindowRequest::ResizePixels {
                    height: screen,
                    width: screen,
                },
            ],
            WindowManipulation::RaiseWin => &[WindowRequest::Raise],
            WindowManipulation::LowerWin => &[WindowRequest::Lower],
            WindowManipulation::RefreshWin => &[WindowRequest::Refresh],
            WindowManipulation::SetWinSizeChars => &[
                WindowRequest::ResizeCells {
                    rows: size,
                    cols: size,
                },
                WindowRequest::ResizeCells {
                    rows: screen,
                    cols: screen,
                },
            ],
            WindowManipulation::MaximizeWin => &[
                WindowRequest::Maximize(Maximize::Restore),
                WindowRequest::Maximize(Maximize::Both),
                WindowRequest::Maximize(Maximize::Vertically),
                WindowRequest::Maximize(Maximize::Horizontally),
            ],
            WindowManipulation::FullscreenWin => &[
                WindowRequest::FullScreen(FullScreen::Leave),
                WindowRequest::FullScreen(FullScreen::Enter),
                WindowRequest::FullScreen(FullScreen::Toggle),
            ],
            WindowManipulation::SetWinLines => &[WindowRequest::ResizeCells {
                rows: size,
                cols: SizeAsk::Keep,
            }],
        };
        forms.iter().all(|&r| does(r))
    }
}

/// Which [`WindowManipulation`]s the embedder carries out. [`NONE`] until
/// it says, so a core no embedder told lists every one as disallowed: it
/// queues the requests and nobody performs them.
///
/// [`NONE`]: WindowAbilities::NONE
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WindowAbilities(u16);

impl WindowAbilities {
    pub const NONE: WindowAbilities = WindowAbilities(0);

    fn bit(m: WindowManipulation) -> u16 {
        let at = WindowManipulation::ALL
            .iter()
            .position(|&x| x == m)
            .unwrap_or(0);
        1 << at
    }

    pub fn with(self, m: WindowManipulation) -> WindowAbilities {
        WindowAbilities(self.0 | WindowAbilities::bit(m))
    }

    pub fn has(self, m: WindowManipulation) -> bool {
        self.0 & WindowAbilities::bit(m) != 0
    }

    /// Every manipulation all of whose forms `does` carries out.
    pub fn of(does: impl Fn(WindowRequest) -> bool) -> WindowAbilities {
        WindowManipulation::ALL
            .into_iter()
            .filter(|m| m.done_by(&does))
            .fold(WindowAbilities::NONE, WindowAbilities::with)
    }
}

/// Whether a program may log what it sends to a file: xterm's `?46`, "Start
/// logging", and `OSC 46`, "Change Log File to Pt" (`[[source: ctlseqs]]`).
/// The embedder's, since only it writes files: `Off` in a core no embedder
/// told, and in the app `On` by default, with `Off` refusing both, counted,
/// as xterm's `logInhibit` "prevent[s] the logging feature from being
/// enabled" (`[[source: xterm-man]]`). A program on the other end of the
/// line, a remote one included, can otherwise append what it sends to any
/// file the user owns that no one else may write.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Logging {
    On,
    #[default]
    Off,
}

/// Whether `?46` has started logging: the mode's state, which `DECRQM`
/// reports and XTSAVE keeps.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum LogMode {
    Started,
    #[default]
    Stopped,
}

/// One dimension of a resize, as ctlseqs reads it for `CSI 4 t` and `CSI 8
/// t`: "Omitted parameters reuse the current height or width. Zero
/// parameters use the display's height or width."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeAsk {
    /// Omitted, or left empty: the window keeps this dimension.
    Keep,
    /// `0`: the screen's, the display's height or width.
    Screen,
    /// This many pixels, or cells.
    Exactly(u16),
}

impl SizeAsk {
    /// The dimension parameter `p` asks for, xterm's `optional_param`:
    /// absent or empty is `DEFAULT`, which keeps the size, and 0 is the
    /// display's (`RequestResize`, `[[source: xterm-reset]]`'s file).
    fn of(p: Option<Param>) -> SizeAsk {
        match p {
            None => SizeAsk::Keep,
            Some(Param(0)) => SizeAsk::Screen,
            Some(Param(n)) => SizeAsk::Exactly(n),
        }
    }
}

/// `CSI 9 ; Ps t`: "0 -> Restore maximized window. 1 -> Maximize window
/// (i.e., resize to screen size). 2 -> Maximize window vertically. 3 ->
/// Maximize window horizontally."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Maximize {
    Restore,
    Both,
    Vertically,
    Horizontally,
}

/// `CSI 10 ; Ps t`: "0 -> Undo full-screen mode. 1 -> Change to
/// full-screen. 2 -> Toggle full-screen."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FullScreen {
    Leave,
    Enter,
    Toggle,
}

/// What a program asks of the window, XTWINOPS 1-10 and DECSLPP
/// (`[[source: ctlseqs]]`): the core queues it and the embedder, which has
/// the window, performs it. Every value is as the program sent it; what
/// "the screen" and "the current size" are is the embedder's to know.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowRequest {
    /// `CSI 1 t`, "De-iconify window".
    Deiconify,
    /// `CSI 2 t`, "Iconify window".
    Iconify,
    /// `CSI 3 ; x ; y t`, "Move window to [x, y]": the window's top-left,
    /// its frame included, in device pixels from the primary screen's
    /// top-left -- where `CSI 13 t` reports it. An omitted or 0 coordinate
    /// is 0, as xterm reads it (`zero_if_default`).
    Move(ScreenPoint),
    /// `CSI 4 ; height ; width t`, "Resize the xterm window to given height
    /// and width in pixels": the text area with its padding, which xterm's
    /// own measure is (`RequestResize` sizes its VT100 widget, the text and
    /// its border).
    ResizePixels { height: SizeAsk, width: SizeAsk },
    /// `CSI 5 t`, "Raise the xterm window to the front of the stacking
    /// order".
    Raise,
    /// `CSI 6 t`, "Lower the xterm window to the bottom of the stacking
    /// order".
    Lower,
    /// `CSI 7 t`, "Refresh the xterm window".
    Refresh,
    /// `CSI 8 ; height ; width t`, "Resize the text area to given height and
    /// width in characters", and DECSLPP, `CSI Ps t` with `Ps` 24 and up,
    /// "Resize to Ps lines", the columns kept, as xterm's
    /// `RequestResize(xw, code, -1, True)` keeps them.
    ResizeCells { rows: SizeAsk, cols: SizeAsk },
    /// `CSI 9 ; Ps t`.
    Maximize(Maximize),
    /// `CSI 10 ; Ps t`.
    FullScreen(FullScreen),
}

impl WindowRequest {
    /// The request `op` makes, with its parameters from `p`; `None` for an
    /// operation that is not one of the window operations.
    pub fn of(op: WindowOp, p: &Params<'_>) -> Option<WindowRequest> {
        let at = |i: u8| p.at(ParamIdx::FIRST.plus(i));
        let second = p.raw(ParamIdx::FIRST.plus(1), 0);
        Some(match op {
            WindowOp::Deiconify => WindowRequest::Deiconify,
            WindowOp::Iconify => WindowRequest::Iconify,
            WindowOp::Move => WindowRequest::Move(ScreenPoint {
                x: ScreenCoord::from_wire(second),
                y: ScreenCoord::from_wire(p.raw(ParamIdx::FIRST.plus(2), 0)),
            }),
            WindowOp::ResizePixels => WindowRequest::ResizePixels {
                height: SizeAsk::of(at(1)),
                width: SizeAsk::of(at(2)),
            },
            WindowOp::Raise => WindowRequest::Raise,
            WindowOp::Lower => WindowRequest::Lower,
            WindowOp::Refresh => WindowRequest::Refresh,
            WindowOp::ResizeCells => WindowRequest::ResizeCells {
                rows: SizeAsk::of(at(1)),
                cols: SizeAsk::of(at(2)),
            },
            WindowOp::Maximize => WindowRequest::Maximize(match second {
                0 => Maximize::Restore,
                1 => Maximize::Both,
                2 => Maximize::Vertically,
                _ => Maximize::Horizontally,
            }),
            WindowOp::FullScreen => WindowRequest::FullScreen(match second {
                0 => FullScreen::Leave,
                1 => FullScreen::Enter,
                _ => FullScreen::Toggle,
            }),
            WindowOp::Lines => WindowRequest::ResizeCells {
                rows: SizeAsk::Exactly(p.raw(ParamIdx::FIRST, 0)),
                cols: SizeAsk::Keep,
            },
            _ => return None,
        })
    }
}
