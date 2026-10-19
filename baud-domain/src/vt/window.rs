//! What an embedder knows about its window and screen, for XTWINOPS's
//! reports (`[[source: ctlseqs]]`, "Window manipulation (XTWINOPS)").
//!
//! The core has no window. Each fact here is one the embedder hands over
//! (`Term::set_window`) or leaves `None`, and a report whose fact is `None`
//! is not answered: silence is the answer an application already handles
//! for a terminal that does not implement the report, and a made-up number
//! is not.

use crate::ImagePx;

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

    /// The coordinate as XTWINOPS writes it: "X Toolkit positions can be
    /// negative, but the reported values are unsigned, in the range
    /// 0-65535. Negative values correspond to 32768-65535"
    /// (`[[source: ctlseqs]]`) -- the low sixteen bits of the two's
    /// complement, so -1 is 65535.
    pub fn wire(self) -> u16 {
        u16::try_from(self.0.rem_euclid(1 << 16)).unwrap_or(u16::MAX)
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
