//! What a program's pointer shape becomes on the window: `OSC 22`, "Change
//! pointer cursor shape to Pt" (`[[source: ctlseqs]]`), one of the 77 names
//! of xterm's cursor-font table, shown as the nearest of AppKit's system
//! cursors.
//!
//! **Nearest, not the same.** X's cursor font draws all 77; AppKit offers a
//! few cursors and resize cursors for a frame's edges and corners and for a
//! column's or a row's divider, so a name with no counterpart -- a "gumby",
//! a "watch" -- shows the arrow. A shape the program never set is the
//! system's own pointer, and nothing is set for it.

use baud_core::PointerShape;

/// The AppKit cursors a shape is shown as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SystemPointer {
    Arrow,
    IBeam,
    PointingHand,
    OpenHand,
    ClosedHand,
    Crosshair,
    NotAllowed,
    ContextualMenu,
    /// A frame resized from one edge or corner, inward or outward.
    FrameResize(Edge),
    /// A column's divider moved left or right.
    ColumnResize,
    /// A row's divider moved up or down.
    RowResize,
}

/// The edge or corner of a frame a resize cursor moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// The system cursor nearest to `shape`: xterm's text pointer is the I-beam,
/// the hands point, the crosses are the crosshair, the "fleur" (move) is
/// the open hand and "sizing" the closed one, "X_cursor" and "pirate" refuse,
/// "question_arrow" asks, a side or a corner resizes the frame from it,
/// "sb_h_double_arrow" moves a column's divider and "sb_v_double_arrow" and
/// "double_arrow" a row's; every other name is the arrow.
pub fn system_pointer(shape: PointerShape) -> SystemPointer {
    use PointerShape as S;
    match shape {
        S::Xterm => SystemPointer::IBeam,
        S::Hand1 | S::Hand2 => SystemPointer::PointingHand,
        S::Fleur => SystemPointer::OpenHand,
        S::Sizing => SystemPointer::ClosedHand,
        S::Crosshair | S::Cross | S::CrossReverse | S::Tcross | S::DiamondCross | S::Plus => {
            SystemPointer::Crosshair
        }
        S::XCursor | S::Pirate => SystemPointer::NotAllowed,
        S::QuestionArrow => SystemPointer::ContextualMenu,
        S::TopSide => SystemPointer::FrameResize(Edge::Top),
        S::BottomSide => SystemPointer::FrameResize(Edge::Bottom),
        S::LeftSide => SystemPointer::FrameResize(Edge::Left),
        S::RightSide => SystemPointer::FrameResize(Edge::Right),
        S::TopLeftCorner => SystemPointer::FrameResize(Edge::TopLeft),
        S::TopRightCorner => SystemPointer::FrameResize(Edge::TopRight),
        S::BottomLeftCorner => SystemPointer::FrameResize(Edge::BottomLeft),
        S::BottomRightCorner => SystemPointer::FrameResize(Edge::BottomRight),
        S::SbHDoubleArrow => SystemPointer::ColumnResize,
        S::SbVDoubleArrow | S::DoubleArrow => SystemPointer::RowResize,
        _ => SystemPointer::Arrow,
    }
}

/// What the window last showed: unknown after the pointer left it, since
/// whatever it crossed may have set another; else the cursor set, or the
/// system's own where no program asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shown {
    Unknown,
    System,
    Set(SystemPointer),
}

/// What to set for `want` -- the core's shape mapped, or `None` for the
/// system's own -- given what is `shown`: nothing when it is already
/// showing, and nothing for the system's own unless a program's shape is
/// showing, so a session that never sends `OSC 22` never sets a cursor.
pub fn to_set(want: Option<SystemPointer>, shown: Shown) -> Option<SystemPointer> {
    match (want, shown) {
        (Some(w), Shown::Set(s)) if w == s => None,
        (Some(w), _) => Some(w),
        (None, Shown::Set(_)) => Some(SystemPointer::Arrow),
        (None, _) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterms_text_pointer_is_the_i_beam_and_the_rest_the_nearest() {
        assert_eq!(system_pointer(PointerShape::Xterm), SystemPointer::IBeam);
        assert_eq!(
            system_pointer(PointerShape::Hand2),
            SystemPointer::PointingHand
        );
        assert_eq!(
            system_pointer(PointerShape::Crosshair),
            SystemPointer::Crosshair
        );
        assert_eq!(
            system_pointer(PointerShape::XCursor),
            SystemPointer::NotAllowed
        );
        assert_eq!(system_pointer(PointerShape::Gumby), SystemPointer::Arrow);
        assert_eq!(system_pointer(PointerShape::Watch), SystemPointer::Arrow);
    }

    /// **Each side, corner and double-arrow name is AppKit's resize
    /// cursor for it**: a side or a corner resizes the frame from that edge
    /// either way, the horizontal double arrow moves a column's divider and
    /// the vertical ones a row's. With the fourteen other names that have a
    /// cursor, twenty-five of the 77 are not the arrow.
    #[test]
    fn each_side_corner_and_double_arrow_is_appkits_resize_cursor() {
        use PointerShape as S;
        use SystemPointer as P;
        let resize = [
            (S::TopSide, P::FrameResize(Edge::Top)),
            (S::BottomSide, P::FrameResize(Edge::Bottom)),
            (S::LeftSide, P::FrameResize(Edge::Left)),
            (S::RightSide, P::FrameResize(Edge::Right)),
            (S::TopLeftCorner, P::FrameResize(Edge::TopLeft)),
            (S::TopRightCorner, P::FrameResize(Edge::TopRight)),
            (S::BottomLeftCorner, P::FrameResize(Edge::BottomLeft)),
            (S::BottomRightCorner, P::FrameResize(Edge::BottomRight)),
            (S::SbHDoubleArrow, P::ColumnResize),
            (S::SbVDoubleArrow, P::RowResize),
            (S::DoubleArrow, P::RowResize),
        ];
        for (name, cursor) in resize {
            assert_eq!(system_pointer(name), cursor, "{name:?}");
        }
        let own = PointerShape::ALL
            .iter()
            .filter(|s| system_pointer(**s) != P::Arrow)
            .count();
        assert_eq!(own, 25);
    }

    /// **The resize cursors are AppKit's current ones**: none of the six
    /// deprecated `resize...Cursor` properties is named here or where the
    /// app sets a cursor, and the frame, column and row cursors are.
    #[test]
    fn no_deprecated_resize_cursor_is_named() {
        const APP: &str = crate::app_source::app_source();
        const OWN: &str = include_str!("pointer.rs");
        for old in [
            concat!("resize", "Left", "Cursor"),
            concat!("resize", "Right", "Cursor"),
            concat!("resize", "LeftRight", "Cursor"),
            concat!("resize", "Up", "Cursor"),
            concat!("resize", "Down", "Cursor"),
            concat!("resize", "UpDown", "Cursor"),
        ] {
            assert!(!APP.contains(old), "app.rs names {old}");
            assert!(!OWN.contains(old), "pointer.rs names {old}");
        }
        for new in [
            concat!("NSCursor::", "frameResizeCursorFromPosition_inDirections("),
            concat!("NSCursor::", "columnResizeCursor()"),
            concat!("NSCursor::", "rowResizeCursor()"),
        ] {
            assert!(APP.contains(new), "app.rs does not call {new}");
        }
    }

    /// **Every system the app runs on has the resize cursors**: AppKit
    /// gives them from macOS 15, and the bundle's minimum system is at or
    /// past it, as is the binary's own (`tests/macos_floor.rs` holds the two
    /// equal), so no system that loads the app takes an availability branch
    /// and the cursors are called with none.
    #[test]
    fn the_bundles_minimum_system_has_the_resize_cursors() {
        const PLIST: &str = include_str!("../../packaging/macos/Info.plist");
        let key = "<key>LSMinimumSystemVersion</key>";
        let major = PLIST
            .split_once(key)
            .and_then(|(_, rest)| rest.split_once("<string>"))
            .and_then(|(_, rest)| rest.split(['.', '<']).next())
            .and_then(|m| m.parse::<u32>().ok());
        assert!(major.is_some_and(|m| m >= 15), "{major:?}");
    }

    /// **A session that never names a shape never sets a cursor**, and a
    /// shape already showing is not set again; the system's own comes back
    /// as the arrow only over a shape a program set.
    #[test]
    fn a_cursor_is_set_only_when_it_changes() {
        let i = SystemPointer::IBeam;
        assert_eq!(to_set(None, Shown::Unknown), None);
        assert_eq!(to_set(None, Shown::System), None);
        assert_eq!(to_set(Some(i), Shown::Unknown), Some(i));
        assert_eq!(to_set(Some(i), Shown::Set(i)), None);
        assert_eq!(
            to_set(Some(SystemPointer::Crosshair), Shown::Set(i)),
            Some(SystemPointer::Crosshair)
        );
        assert_eq!(to_set(None, Shown::Set(i)), Some(SystemPointer::Arrow));
    }
}
