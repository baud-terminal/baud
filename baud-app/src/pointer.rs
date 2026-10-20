//! What a program's pointer shape becomes on the window: `OSC 22`, "Change
//! pointer cursor shape to Pt" (`[[source: ctlseqs]]`), one of the 77 names
//! of xterm's cursor-font table, shown as the nearest of AppKit's system
//! cursors.
//!
//! **Nearest, not the same.** X's cursor font draws all 77; AppKit offers a
//! handful, and the resize cursors among them are deprecated, so a name
//! with no counterpart -- a "gumby", a "watch", a resize arrow -- shows the
//! arrow. A shape the program never set is the system's own pointer, and
//! nothing is set for it.

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
}

/// The system cursor nearest to `shape`: xterm's text pointer is the I-beam,
/// the hands point, the crosses are the crosshair, the "fleur" (move) is
/// the open hand and "sizing" the closed one, "X_cursor" and "pirate" refuse,
/// "question_arrow" asks; every other name is the arrow.
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
        assert_eq!(
            system_pointer(PointerShape::SbHDoubleArrow),
            SystemPointer::Arrow
        );
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
