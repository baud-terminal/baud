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
    /// * The *position* has to be encodable. X10 -- the default, and what
    ///   `?1000` gives without `?1006` -- caps each ordinate at 223 against a
    ///   2048-column grid, so a report from column 400 has no encoding: the
    ///   byte it would produce names a different cell rather than a truncated
    ///   one, which is the wrong-shaped answer `docs/vt-coverage.md` rules out.
    ///   Those are dropped and counted in `mouse_unencodable`. Everything that
    ///   fits is sent.
    ///
    ///   This used to report *nothing at all* without `?1006`, on that same
    ///   reasoning. It is a reason not to encode out of range, not a reason to
    ///   refuse the encoding: an application speaking the original protocol on
    ///   an 80x24 screen was getting silence where every byte was encodable.
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
        // The encoding decides whether this *position* can be named. X10 caps
        // each ordinate at 223, so a report from column 400 has no encoding --
        // and the byte it would produce points at a different cell rather than
        // a truncated one. Refused and counted; the positions that fit are
        // sent, which is every one on a terminal anyone actually runs.
        if let Some(max) = self.mouse_encoding.max_ordinate() {
            let (r, c) = (e.row.one_based().get(), e.col.one_based().get());
            if r > max || c > max {
                self.mouse_unencodable.inc();
                return false;
            }
            self.report(Report::MouseX10(e));
            return true;
        }
        self.report(Report::MouseSgr(e));
        true
    }
}
