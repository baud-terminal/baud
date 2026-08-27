use crate::*;

impl<'a> Term<'a> {
    pub(crate) fn set_tracking(&mut self, level: MouseTracking, on: bool) {
        self.mouse = if on { level } else { MouseTracking::Off };
    }

    /// What tracking the application asked for. `?1000/1002/1003`.
    pub fn mouse_tracking(&self) -> MouseTracking {
        self.mouse
    }

    /// Queue a mouse report, if the application asked for this kind of event.
    ///
    /// Returns whether anything was queued, so an embedder can tell "the
    /// application does not want this" from "it was sent".
    ///
    /// Two gates, and both are refusals rather than omissions:
    ///
    /// * The tracking level decides which *kinds* are reported. Motion is
    ///   dropped under `?1000`, and motion with no button held is dropped under
    ///   anything below `?1003` -- sending it anyway would flood an application
    ///   that asked only for clicks.
    /// * Without `?1006` nothing is reported at all, and `mouse_unencodable`
    ///   counts it. The X10 encoding caps each ordinate at 223 and baud's grid
    ///   goes to 2048, so a report from column 400 would not be truncated, it
    ///   would point at the wrong cell. That is the wrong-shaped answer
    ///   `docs/vt-coverage.md` rules out.
    pub fn mouse_report(&mut self, e: MouseEvent) -> bool {
        let wanted = match (self.mouse, e.kind) {
            (MouseTracking::Off, _) => false,
            (MouseTracking::Normal, MouseKind::Motion) => false,
            (MouseTracking::ButtonEvent, MouseKind::Motion) => e.button != MouseButton::None,
            _ => true,
        };
        if !wanted {
            return false;
        }
        if self.mouse_encoding == MouseEncoding::None {
            self.mouse_unencodable.inc();
            return false;
        }
        self.report(Report::MouseSgr(e));
        true
    }
}
