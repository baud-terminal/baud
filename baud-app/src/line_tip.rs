//! **The hover text over DEC's indicator line's words**: the state
//! lights' words (`line_lamps.rs`) mean what the title strip's lights do,
//! and the pointer over one shows the same text the strip's tooltip does,
//! `Led::tip`, drawn by baud in the frame on macOS and Linux alike
//! (`baud_render::tip`): Wayland has no tooltip protocol, and drawn here
//! the pixels are the renderer's on both systems.
//!
//! **And over the lights on their own line, on Linux**, all nine: the
//! pointer over a light's place on the line ([`LightsLine`], where the
//! last frame drew them) shows its `Led::tip` the same way, above a line
//! at the bottom and below one at the top, by the same rules. On macOS
//! those lights carry AppKit's tooltips (`lights::LineTips`).
//!
//! **WCAG 2.2's 1.4.13, Content on Hover or Focus** (`[[source: wcag22]]`)
//! governs a tip baud draws itself; its exception, for content "controlled
//! by the user agent", covers the strip's AppKit tooltips and not this:
//!
//! * **Hoverable**: "the pointer can be moved over the additional content
//!   without the additional content disappearing". The tip stays while the
//!   pointer is over its word or anywhere in its box, the gap row between
//!   the box and the line included. A light's tip stays from its light up
//!   or down to the box and across it, over whatever lies between -- the
//!   line's margin, the notice, DEC's line, the gutter -- in the box's
//!   columns and the light's.
//! * **Dismissible**: "A mechanism is available to dismiss the additional
//!   content without moving pointer hover", since it covers the grid's
//!   cells. A key typed dismisses it, and still goes to the program, which
//!   owns every key, Escape too; it stays dismissed while the pointer stays
//!   on that word, and the next word, or the same one come back to, shows
//!   its own.
//! * **Persistent**: "remains visible until the hover or focus trigger is
//!   removed, the user dismisses it, or its information is no longer
//!   valid". No timer takes it away. It goes when the pointer leaves the
//!   word and the box, when a key dismisses it, or when its word leaves
//!   the line -- its lamp gone dark, the line closed, the window too narrow
//!   for the word; a light's when its light leaves the line. A tip its
//!   word's leaving took stays down until the pointer moves, so a
//!   condition that comes and goes fast shows the box once and never
//!   flashes it.
//!
//! What it does not reach: keyboard focus, since a terminal's keys are the
//! program's, and a screen reader, since baud exposes no accessibility
//! element for the grid on either system. The strip's lights on macOS each
//! carry their hover text as their accessibility help as well
//! (`lights.rs`).

use baud_core::{IndicatorLamp, Term};
use baud_domain::{Col, Row, Rows};
use baud_font::Px;
use baud_render::{ByLed, Led, LineEdge, TipArea, TipLook};

/// **Where the pointer is, for the tip**: over the main display's cell
/// `row`, `col`; over column `col` of the status line's row; or anywhere
/// else, off the window's text and over its padding included.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Over {
    Main(Row, Col),
    Line(Col),
    Elsewhere,
}

/// **The cell under a view point, the status line's row included**:
/// [`crate::gesture::pointer_under`]'s arithmetic, unclamped, over `grid`
/// columns and rows of the main display and the `status` rows below it.
/// A point past the main display's last row is on the line, where the
/// pointer for a selection or a report is clamped to the last row.
pub(crate) fn over_at(
    at: (f64, f64),
    scale: f64,
    cell: (f64, f64),
    grid: (u16, u16),
    pad: (u32, u32),
    status: Rows,
) -> Over {
    let index = |p: f64, pad: u32, cell: f64| {
        let px = (p * scale - f64::from(pad)).floor();
        match px >= 0.0 && cell > 0.0 {
            true => usize::try_from((px / cell).floor() as i64).ok(),
            false => None,
        }
    };
    let (Some(col), Some(row)) = (index(at.0, pad.0, cell.0), index(at.1, pad.1, cell.1)) else {
        return Over::Elsewhere;
    };
    let (cols, rows) = (usize::from(grid.0), usize::from(grid.1));
    match (col < cols, row) {
        (false, _) => Over::Elsewhere,
        (true, r) if r < rows => Over::Main(Row::from_index(r), Col::at_index(col)),
        (true, r) if r < rows + status.index() => Over::Line(Col::at_index(col)),
        (true, _) => Over::Elsewhere,
    }
}

/// The strip's light a word on the line is.
pub(crate) fn led(lamp: IndicatorLamp) -> Led {
    match lamp {
        IndicatorLamp::CarrierDetect => Led::Cd,
        IndicatorLamp::OffHook => Led::Oh,
        IndicatorLamp::ClearToSend => Led::Cs,
        IndicatorLamp::Fax => Led::Fax,
        IndicatorLamp::Synchronous => Led::Syn,
    }
}

/// Whether what the frame draws changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Changed {
    No,
    Yes,
}

/// **What a tip explains**: a word on DEC's indicator line, or a light on
/// the lights' own line where baud draws that light's tip (Linux).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Subject {
    Word(IndicatorLamp),
    Light(Led),
}

impl Subject {
    /// The light whose text the tip shows.
    fn led(self) -> Led {
        match self {
            Subject::Word(l) => led(l),
            Subject::Light(l) => l,
        }
    }
}

/// **The lights' own line as the last frame drew it**, in the whole
/// frame's device pixels, for a light's tip where baud draws it: the edge
/// it is at, its top and height, each placed light's left and width
/// (`Renderer::lights_places`), where the grid's first cell starts, a
/// cell's size, and the whole frame's width. Only the Linux driver makes
/// one.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct LightsLine {
    pub(crate) edge: LineEdge,
    pub(crate) top: Px,
    pub(crate) height: Px,
    pub(crate) places: ByLed<Option<(Px, Px)>>,
    pub(crate) origin: (Px, Px),
    pub(crate) cell: (Px, Px),
    pub(crate) width: Px,
}

impl LightsLine {
    /// **The light whose place on the line `p` is in**, a device pixel of
    /// the whole frame; none off the line or between the lights.
    pub(crate) fn light_at(&self, p: (f64, f64)) -> Option<Led> {
        let px = |v: Px| f64::from(v.value());
        let on_line = (px(self.top)..px(self.top) + px(self.height)).contains(&p.1);
        Led::ALL.into_iter().filter(|_| on_line).find(|&led| {
            self.places
                .get(led)
                .is_some_and(|(x, w)| (px(x)..px(x) + px(w)).contains(&p.0))
        })
    }

    /// **Whether `p` is in `led`'s tip's reach**, its box at `area`: in
    /// the box's columns or the light's, from the box to the line, so the
    /// pointer crosses from the light onto the box over the notice, DEC's
    /// line and the gutter, and across the box, without the tip going. On
    /// the line itself only the light's own place holds it, as on DEC's
    /// line only the word does.
    fn reach(&self, area: TipArea, led: Led, p: (f64, f64)) -> bool {
        let px = |v: Px| f64::from(v.value());
        let cell = |o: Px, size: Px, i: usize| px(o) + px(size) * i as f64;
        let box_left = cell(self.origin.0, self.cell.0, area.left.index());
        let left = self
            .places
            .get(led)
            .map_or(box_left, |(x, _)| box_left.min(px(x)));
        let (top, bottom) = match self.edge {
            LineEdge::Bottom => (
                cell(self.origin.1, self.cell.1, area.top.index()),
                px(self.top),
            ),
            LineEdge::Top => (
                px(self.top) + px(self.height),
                cell(self.origin.1, self.cell.1, area.bottom.index()),
            ),
        };
        (left..px(self.width)).contains(&p.0) && (top..bottom).contains(&p.1)
    }
}

/// **The tip's state**: where the pointer is, the word or light whose tip
/// shows and what it draws, the one a key dismissed while the pointer is
/// on it, and the one its own leaving took down while the pointer was
/// still. A driver updates it on each pointer event, refreshes it at each
/// frame, dismisses it on each key, and hands [`LineTip::look`] to the
/// renderer; the Linux driver also gives it the lights' line as drawn.
#[derive(Default)]
pub(crate) struct LineTip {
    over: Option<Over>,
    /// The pointer in the whole frame's device pixels, where the driver
    /// draws the lights' line's tips.
    at: Option<(f64, f64)>,
    lights: Option<LightsLine>,
    subject: Option<Subject>,
    look: Option<TipLook>,
    dismissed: Option<Subject>,
    went: Option<Subject>,
}

impl LineTip {
    /// The pointer moved to `over`.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn update(&mut self, term: &Term<'_>, over: Over) -> Changed {
        self.update_at(term, over, None)
    }

    /// The pointer moved to `over`, at `at` in the whole frame's device
    /// pixels where the driver draws the lights' line's tips.
    pub(crate) fn update_at(
        &mut self,
        term: &Term<'_>,
        over: Over,
        at: Option<(f64, f64)>,
    ) -> Changed {
        self.went = None;
        self.over = Some(over);
        self.at = at;
        self.settle(term)
    }

    /// The pointer left the window: what AppKit's exit says; the Linux
    /// seat reads a pointer off the surface as [`Over::Elsewhere`].
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn leave(&mut self, term: &Term<'_>) -> Changed {
        self.went = None;
        self.over = None;
        self.at = None;
        self.settle(term)
    }

    /// The lights' own line as the last frame drew it, or none: where a
    /// light's tip is hit and whether its light is still placed. The
    /// pointer has not moved, so nothing else changes.
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn set_lights(&mut self, lights: Option<LightsLine>) {
        self.lights = lights;
    }

    /// **A frame under a pointer that has not moved**: its word may have
    /// left the line, a light the line, or the window changed size. A key's
    /// dismissal stands, since the pointer is where it was, and a tip this
    /// takes down stays down until the pointer moves: a condition that
    /// comes and goes at the frame's pace shows the box once, not 30 times
    /// a second (WCAG 2.3.1).
    pub(crate) fn refresh(&mut self, term: &Term<'_>) -> Changed {
        let (shown, dismissed) = (self.subject, self.dismissed);
        let changed = self.settle(term);
        self.dismissed = self.dismissed.or(dismissed);
        if let (Some(s), None) = (shown, self.subject) {
            self.went = Some(s);
        }
        changed
    }

    /// A key was typed: the tip shown goes, until the pointer comes to a
    /// word again.
    pub(crate) fn dismiss(&mut self, term: &Term<'_>) -> Changed {
        if let Some(s) = self.subject {
            self.dismissed = Some(s);
        }
        self.settle(term)
    }

    /// What the next frame draws, or none.
    pub(crate) fn look(&self) -> Option<&TipLook> {
        self.look.as_ref()
    }

    fn settle(&mut self, term: &Term<'_>) -> Changed {
        let light = match (self.lights, self.at) {
            (Some(line), Some(p)) => line.light_at(p),
            _ => None,
        };
        let under = match (self.over, light) {
            (Some(Over::Line(x)), _) => term.indicator_lamp_at(x).map(Subject::Word),
            (_, Some(l)) => Some(Subject::Light(l)),
            _ => None,
        };
        // Its word still on the line, or its light: the information valid.
        let lights = self.lights;
        let valid = |s: Subject| match s {
            Subject::Word(l) => (0..term.geometry().0.index())
                .any(|x| term.indicator_lamp_at(Col::at_index(x)) == Some(l)),
            Subject::Light(l) => lights.is_some_and(|line| line.places.get(l).is_some()),
        };
        let (over, at) = (self.over, self.at);
        let in_reach = |look: &Option<TipLook>, subject: Option<Subject>| match (look, subject) {
            (Some(t), Some(Subject::Word(_))) => {
                matches!(over, Some(Over::Main(r, c)) if t.area.holds(r, c))
            }
            (Some(t), Some(Subject::Light(l))) => match (lights, at) {
                (Some(line), Some(p)) => line.reach(t.area, l, p),
                _ => false,
            },
            _ => false,
        };
        let want = match (under, self.subject) {
            // A light's tip holds while the pointer is in its reach, on its
            // way to the box, even across a word of DEC's line in the
            // light's columns: the word's own tip shows only once the
            // pointer has left the light's reach.
            (Some(Subject::Word(_)), Some(Subject::Light(l)))
                if in_reach(&self.look, Some(Subject::Light(l)))
                    && valid(Subject::Light(l))
                    && self.dismissed != Some(Subject::Light(l)) =>
            {
                Some(Subject::Light(l))
            }
            (Some(s), _) if self.dismissed == Some(s) || self.went == Some(s) => None,
            (Some(s), _) => {
                self.dismissed = None;
                Some(s)
            }
            (None, Some(s))
                if in_reach(&self.look, Some(s)) && valid(s) && self.dismissed != Some(s) =>
            {
                Some(s)
            }
            (None, _) => {
                if !in_reach(&self.look, self.subject) {
                    self.dismissed = None;
                }
                None
            }
        };
        let (cols, rows) = term.geometry();
        let opens = match lights.map(|line| line.edge) {
            Some(LineEdge::Top) => baud_render::Opens::Down,
            Some(LineEdge::Bottom) | None => baud_render::Opens::Up,
        };
        let look = want.and_then(|s| match s {
            Subject::Word(_) => baud_render::tip_look(s.led().tip(), cols, rows),
            Subject::Light(_) => baud_render::tip_look_opening(s.led().tip(), cols, rows, opens),
        });
        self.subject = want.filter(|_| look.is_some());
        let changed = look != self.look;
        self.look = look;
        match changed {
            true => Changed::Yes,
            false => Changed::No,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{IndicatorLamps, LampState, Owned, ReadLamp};
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 24,
        ws_col: 80,
    };

    fn feed(t: &mut Term<'_>, b: &[u8]) {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }

    fn lit() -> IndicatorLamps {
        IndicatorLamps {
            cd: ReadLamp::Unknown,
            oh: ReadLamp::Read(LampState::Lit),
            cs: LampState::Dark,
            fax: LampState::Dark,
            syn: LampState::Lit,
        }
    }

    fn text(look: &TipLook) -> String {
        look.lines
            .iter()
            .map(|f| {
                f.text
                    .iter()
                    .filter_map(|c| char::from_u32(c.value()))
                    .collect::<String>()
                    .trim()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// **Both drivers drive the tip**: the macOS view's pointer events,
    /// the line's row counted and an exit leaving it, its keys dismissing
    /// it, each frame refreshing it and handing it to the renderer, and
    /// the cell path drawing its lines with the link preview's bar; the
    /// Linux seat's pointer, its keys, each turn and each frame the same.
    /// The Linux driver gives the tip the lights' own line as drawn, with
    /// the pointer's pixel, so a light there shows its own text. The
    /// strip's lights carry the same text as their accessibility help.
    /// Read off the files, as the lights' wiring is.
    #[test]
    fn both_drivers_drive_the_tip() {
        const MAC: &str = include_str!("app.rs");
        const LINUX: &str = include_str!("linux.rs");
        const STRIP: &str = include_str!("lights.rs");
        let body = |src: &'static str, head: &str| {
            let at = src.find(head).unwrap_or_else(|| panic!("{head}"));
            let rest = &src[at..];
            rest.split_once("\n    }\n").map_or(rest, |(b, _)| b)
        };
        let mouse = body(MAC, concat!("    fn mouse(&mut self, ", "event: &Mouse) {"));
        for want in [
            "ViewMouseKind::Exited => self.app.line_tip.leave(&self.app.term),",
            "crate::line_tip::over_at((event.x, event.y), scale, cell, grid, pad, status)",
            "self.app.line_tip.update(&self.app.term, over)",
        ] {
            assert!(mouse.contains(want), "{want}");
        }
        let key = body(
            MAC,
            concat!("    fn key(&mut self, ", "key: &KeyPress) -> Handled {"),
        );
        assert!(key.contains("self.app.line_tip.dismiss(&self.app.term)"));
        for want in [
            "if self.line_tip.refresh(&self.term) == crate::line_tip::Changed::Yes {",
            "self.renderer.set_tip(self.line_tip.look().cloned());",
            ".tip_bars()",
            ".chain(tips)",
            "self.line_tip.refresh(&self.term) == crate::line_tip::Changed::Yes",
            "self.seat.line_over(g, self.term.status_rows())",
            "self.line_tip.dismiss(&self.term)",
        ] {
            assert!(MAC.contains(want) || LINUX.contains(want), "{want}");
        }
        for want in [
            "let at = self.seat.frame_point(g.scale);",
            "self.line_tip.update_at(&self.term, over, at)",
        ] {
            assert_eq!(LINUX.matches(want).count(), 1, "{want}");
        }
        assert_eq!(
            LINUX
                .matches("self.line_tip.set_lights(self.lights_drawn());")
                .count(),
            2,
            "at the pointer and at each turn"
        );
        let drawn = body(
            LINUX,
            concat!(
                "    fn lights_drawn(&self) -> ",
                "Option<crate::line_tip::LightsLine> {"
            ),
        );
        for want in [
            "edge: r.lights_edge()?,",
            "top: r.lights_top(),",
            "places: r.lights_places(),",
            "origin: (r.strip(), r.grid_top()),",
            "width: r.whole_size().0,",
        ] {
            assert!(drawn.contains(want), "{want}");
        }
        let pointer = body(
            LINUX,
            concat!(
                "    fn pointer(&mut self, ",
                "p: PointerEvent, mods: baud_domain::Modifiers) {"
            ),
        );
        assert!(pointer.contains("self.seat.line_over(g, self.term.status_rows())"));
        let route = body(
            LINUX,
            concat!(
                "    fn route_key(&mut self, ",
                "route: crate::pressed::Route) {"
            ),
        );
        assert!(route.contains("self.line_tip.dismiss(&self.term)"));
        let present = body(LINUX, concat!("    fn present(&mut self", ") {"));
        assert!(present.contains("self.renderer.set_tip(self.line_tip.look().cloned());"));
        let turn = body(LINUX, concat!("    fn show_line_lamps(&mut self", ") {"));
        assert!(turn.contains("self.line_tip.refresh(&self.term)"));
        for want in [
            "v.setAccessibilityLabel(Some(&NSString::from_str(led.letters())));",
            "v.setAccessibilityHelp(Some(&NSString::from_str(led.tip())));",
        ] {
            assert!(STRIP.contains(want), "{want}");
        }
    }

    /// **The cell under a point counts the status line's row**: 8 x 17
    /// cells at a scale of one, 80 x 24 and the line below; the last main
    /// row at column 62 is the main display's, the next row the line's,
    /// the padding and past the line elsewhere.
    #[test]
    fn the_cell_under_a_point_counts_the_lines_row() {
        let (one, none) = (Rows::of_count(1), Rows::ZERO);
        let g = |x: f64, y: f64, status: Rows| {
            over_at((x, y), 1.0, (8.0, 17.0), (80, 24), (0, 0), status)
        };
        assert_eq!(
            g(61.0 * 8.0, 23.0 * 17.0, one),
            Over::Main(Row::from_index(23), Col::at_index(61))
        );
        assert_eq!(
            g(61.0 * 8.0, 24.0 * 17.0, one),
            Over::Line(Col::at_index(61))
        );
        assert_eq!(
            g(61.0 * 8.0 + 7.9, 24.0 * 17.0 + 16.9, one),
            Over::Line(Col::at_index(61))
        );
        assert_eq!(
            g(61.0 * 8.0, 24.0 * 17.0, none),
            Over::Elsewhere,
            "no line shown"
        );
        assert_eq!(g(61.0 * 8.0, 25.0 * 17.0, one), Over::Elsewhere);
        assert_eq!(g(80.0 * 8.0, 24.0 * 17.0, one), Over::Elsewhere);
        assert_eq!(g(-1.0, 24.0 * 17.0, one), Over::Elsewhere);
        // At 2x, with the grid padded 4 pixels down.
        let at2 = over_at(
            (488.0 / 2.0, (4.0 + 24.0 * 34.0) / 2.0),
            2.0,
            (16.0, 34.0),
            (80, 24),
            (0, 4),
            one,
        );
        assert_eq!(at2, Over::Line(Col::at_index(30)));
    }

    /// **A word's tip is the strip's text, over the word**: the pointer on
    /// CD's columns shows CD's tooltip, `Led::tip`, the same words the
    /// strip shows; on the space after it, on the last main row above it,
    /// or over a dark lamp's blank columns, none.
    #[test]
    fn a_words_tip_is_the_strips_text_over_the_word() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[1$~");
        t.set_indicator_lamps(Some(lit()));
        let mut tip = LineTip::default();
        assert_eq!(
            tip.update(&t, Over::Main(Row::from_index(23), Col::at_index(61))),
            Changed::No
        );
        assert!(tip.look().is_none(), "the last main row is not the line");
        assert_eq!(tip.update(&t, Over::Line(Col::at_index(61))), Changed::Yes);
        assert_eq!(text(tip.look().expect("CD's tip")), Led::Cd.tip());
        assert_eq!(
            tip.update(&t, Over::Line(Col::at_index(63))),
            Changed::No,
            "CD?'s mark"
        );
        assert_eq!(tip.update(&t, Over::Line(Col::at_index(64))), Changed::Yes);
        assert!(tip.look().is_none(), "the space after it");
        assert_eq!(tip.update(&t, Over::Line(Col::at_index(66))), Changed::Yes);
        assert_eq!(text(tip.look().expect("OH's tip")), Led::Oh.tip());
        assert_eq!(tip.update(&t, Over::Line(Col::at_index(69))), Changed::Yes);
        assert!(tip.look().is_none(), "CS is dark: nothing is drawn there");
        tip.update(&t, Over::Line(Col::at_index(77)));
        assert_eq!(text(tip.look().expect("SYN's tip")), Led::Syn.tip());
        for lamp in IndicatorLamp::ALL {
            assert!(led(lamp).letters().as_bytes() == lamp.letters(), "{lamp:?}");
        }
    }

    /// **WCAG 2.2's 1.4.13**: hoverable, the pointer moves from the word up
    /// across the gap row onto the box and the tip stays; persistent, no
    /// step takes it away while the pointer stays and the word is drawn,
    /// and it goes when its word leaves the line; dismissible, a key takes
    /// it away while the pointer stays on the word, and the next word shows
    /// its own.
    #[test]
    fn the_tip_is_hoverable_persistent_and_dismissible() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[1$~");
        t.set_indicator_lamps(Some(lit()));
        let mut tip = LineTip::default();
        tip.update(&t, Over::Line(Col::at_index(62)));
        let area = tip.look().expect("CD's tip").area;
        assert_eq!(area.bottom, Row::from_index(24), "down to the line");
        assert!(
            area.holds(Row::from_index(23), Col::at_index(62)),
            "the gap row"
        );
        // Up from the word, across the gap, onto the box and over it.
        for (r, c) in [
            (23, 62),
            (22, 62),
            (21, 40),
            (area.top.index(), area.left.index()),
        ] {
            assert_eq!(
                tip.update(&t, Over::Main(Row::from_index(r), Col::at_index(c))),
                Changed::No,
                "{r},{c}"
            );
            assert!(tip.look().is_some(), "{r},{c}");
        }
        for _ in 0..100 {
            assert_eq!(tip.refresh(&t), Changed::No, "no timer takes it away");
        }
        // Off the box: gone.
        let left = area.left.index() - 1;
        assert_eq!(
            tip.update(&t, Over::Main(Row::from_index(22), Col::at_index(left))),
            Changed::Yes
        );
        assert!(tip.look().is_none());
        // Its word leaves the line: the information no longer valid.
        tip.update(&t, Over::Line(Col::at_index(62)));
        tip.update(&t, Over::Main(Row::from_index(22), Col::at_index(62)));
        assert!(tip.look().is_some());
        t.set_indicator_lamps(Some(IndicatorLamps {
            cd: ReadLamp::Read(LampState::Dark),
            ..lit()
        }));
        assert_eq!(tip.refresh(&t), Changed::Yes);
        assert!(tip.look().is_none(), "CD went dark");
        // Dismissed by a key, on the word; the next word shows its own.
        t.set_indicator_lamps(Some(lit()));
        tip.update(&t, Over::Line(Col::at_index(61)));
        assert_eq!(tip.dismiss(&t), Changed::Yes);
        assert!(tip.look().is_none());
        assert_eq!(
            tip.update(&t, Over::Line(Col::at_index(62))),
            Changed::No,
            "stays dismissed"
        );
        assert_eq!(tip.update(&t, Over::Line(Col::at_index(66))), Changed::Yes);
        assert_eq!(text(tip.look().expect("OH's")), Led::Oh.tip());
        tip.update(&t, Over::Line(Col::at_index(61)));
        assert_eq!(text(tip.look().expect("CD's again")), Led::Cd.tip());
        // The line closed takes it away; the pointer leaving does too.
        feed(&mut t, b"\x1b[0$~");
        assert_eq!(tip.refresh(&t), Changed::Yes);
        assert!(tip.look().is_none());
        feed(&mut t, b"\x1b[1$~");
        tip.update(&t, Over::Line(Col::at_index(61)));
        assert_eq!(tip.leave(&t), Changed::Yes);
        assert!(tip.look().is_none());
    }

    /// **A word that comes and goes under a still pointer shows its box
    /// once**: OH lit and dark on alternate frames, as a loop of short
    /// commands makes it, with the pointer held on OH's columns, is at most
    /// three shows in 60 frames -- here none after the first, where the tip
    /// came back with its word 29 times before -- and a key's dismissal
    /// holds the same way; the pointer moving shows it again. WCAG 2.3.1:
    /// the box is no flash. Perturbation: drop the rule from the first arm
    /// and the count is 29.
    #[test]
    fn a_word_that_comes_and_goes_under_a_still_pointer_shows_once() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[1$~");
        let dark = IndicatorLamps {
            oh: ReadLamp::Read(LampState::Dark),
            ..lit()
        };
        let flip = |t: &mut Term<'_>, tip: &mut LineTip| {
            let mut shows = 0;
            for k in 0..60 {
                t.set_indicator_lamps(Some(if k % 2 == 0 { lit() } else { dark }));
                let was = tip.look().is_some();
                tip.refresh(t);
                shows += usize::from(!was && tip.look().is_some());
            }
            shows
        };
        let mut tip = LineTip::default();
        t.set_indicator_lamps(Some(lit()));
        tip.update(&t, Over::Line(Col::at_index(66)));
        assert!(tip.look().is_some(), "the control: OH's tip");
        let shows = flip(&mut t, &mut tip);
        assert!(shows <= 3, "{shows} shows in 60 frames");
        // The pointer moves on the word: its tip again.
        t.set_indicator_lamps(Some(lit()));
        tip.update(&t, Over::Line(Col::at_index(65)));
        assert_eq!(text(tip.look().expect("OH's again")), Led::Oh.tip());
        // Dismissed by a key, it stays dismissed through the flips.
        tip.dismiss(&t);
        assert_eq!(flip(&mut t, &mut tip), 0, "a dismissal holds");
        // The tip over its box, the pointer still: the same.
        t.set_indicator_lamps(Some(lit()));
        tip.update(&t, Over::Line(Col::at_index(64)));
        tip.update(&t, Over::Line(Col::at_index(66)));
        tip.update(&t, Over::Main(Row::from_index(22), Col::at_index(70)));
        assert!(tip.look().is_some(), "over the box");
        assert!(flip(&mut t, &mut tip) <= 3, "from the box");
    }

    /// The lights' line of a 2x frame 80 x 24 cells of 16 x 34 pixels, the
    /// line at `edge`, 30 pixels tall, its nine lights 40 pixels wide each
    /// two apart from 900, and 44 pixels between the grid and a line at the
    /// bottom -- a notice's row and the gutter -- or 10 between a line at
    /// the top and the grid.
    fn line_at(edge: LineEdge) -> LightsLine {
        let places = ByLed::of(|led| {
            let i = Led::ALL.iter().position(|&l| l == led).unwrap();
            Some((Px::of(900 + 42 * u32::try_from(i).unwrap()), Px::of(40)))
        });
        let (top, origin) = match edge {
            LineEdge::Bottom => (Px::of(24 * 34 + 44), Px::of(0)),
            LineEdge::Top => (Px::of(0), Px::of(40)),
        };
        LightsLine {
            edge,
            top,
            height: Px::of(30),
            places,
            origin: (Px::of(0), origin),
            cell: (Px::of(16), Px::of(34)),
            width: Px::of(1290),
        }
    }

    /// The pointer at device pixel `p` of `line`'s frame, as the Linux
    /// driver hands it on: the cell `over_at` finds and the pixel.
    fn point(tip: &mut LineTip, t: &Term<'_>, line: LightsLine, p: (f64, f64)) -> Changed {
        let over = over_at(
            (p.0 / 2.0, p.1 / 2.0),
            2.0,
            (16.0, 34.0),
            (80, 24),
            (0, line.origin.1.value()),
            t.status_rows(),
        );
        tip.update_at(t, over, Some(p))
    }

    /// **Each light on its own line shows its own tip, all nine**: the
    /// pointer at the middle of each light's place on a line at the bottom
    /// shows that light's `Led::tip`, the four DEC's line never carries
    /// among them, above the line; between two lights on the line, under
    /// it, on the grid, and with no line drawn, none. Perturbation: hit-test against the line's
    /// top alone, not its height, and the pointer under the line still
    /// shows a tip.
    #[test]
    fn each_light_on_its_line_shows_its_own_tip() {
        let mut owned = Owned::new(&WS);
        let t = Term::new(owned.storage(), &WS);
        let line = line_at(LineEdge::Bottom);
        let on = f64::from(line.top.value() + 15);
        let mut tip = LineTip::default();
        tip.set_lights(Some(line));
        for led in Led::ALL {
            let (x, w) = line.places.get(led).unwrap();
            let p = (f64::from(x.value() + w.value() / 2), on);
            point(&mut tip, &t, line, p);
            let look = tip.look().unwrap_or_else(|| panic!("{led:?}'s tip"));
            assert_eq!(text(look), led.tip(), "{led:?}");
            assert_eq!(
                look.area.bottom,
                Row::from_index(24),
                "{led:?}: above the line"
            );
        }
        point(&mut tip, &t, line, (941.0, on));
        assert!(tip.look().is_none(), "between HS and AA, on the line");
        point(&mut tip, &t, line, (920.0, on + 15.0));
        assert!(tip.look().is_none(), "under the line");
        point(&mut tip, &t, line, (920.0, 400.0));
        assert!(tip.look().is_none(), "on the grid");
        let mut none = LineTip::default();
        point(&mut none, &t, line, (920.0, on));
        assert!(
            none.look().is_none(),
            "no line drawn: macOS's AppKit tooltips"
        );
    }

    /// **A light's tip is hoverable, dismissible and persistent as a
    /// word's**: from RD's middle straight up, pixel row by pixel row, off
    /// the line, over the notice and the gutter between it and the grid and
    /// over the gap row into the box and across it, the
    /// tip stays; past the box's left and the light's it goes; a key takes
    /// it away while the pointer stays and the next light shows its own;
    /// it stays through frames whatever the lights show, goes when a narrow
    /// window leaves its light out, and stays down when it comes back until
    /// the pointer moves. On a line at the top the box opens down from it,
    /// and the walk goes down. Perturbation: give a light's tip the box's
    /// cells alone as its reach -- the walk loses it between the line and
    /// the box -- and this goes red.
    #[test]
    fn a_lights_tip_is_hoverable_dismissible_and_persistent() {
        let mut owned = Owned::new(&WS);
        let t = Term::new(owned.storage(), &WS);
        for edge in [LineEdge::Bottom, LineEdge::Top] {
            let line = line_at(edge);
            let mut tip = LineTip::default();
            tip.set_lights(Some(line));
            let (x, w) = line.places.rd.unwrap();
            let mid = f64::from(x.value() + w.value() / 2);
            let on = f64::from(line.top.value() + 15);
            point(&mut tip, &t, line, (mid, on));
            let look = tip.look().expect("RD's tip").clone();
            let area = look.area;
            // A row's top and a column's left, in the frame's pixels.
            let n = |i: usize| u32::try_from(i).unwrap();
            let row_y = |r: Row| line.origin.1.value() + n(r.index()) * 34;
            let ys: Vec<u32> = match edge {
                LineEdge::Bottom => {
                    assert_eq!(area.bottom, Row::from_index(24), "up from the line");
                    (row_y(area.top)..line.top.value() + 15).rev().collect()
                }
                LineEdge::Top => {
                    assert_eq!(area.top, Row::from_index(0), "down from the line");
                    assert_eq!(look.lines[0].row, Row::from_index(1), "a row below");
                    (15..row_y(area.bottom)).collect()
                }
            };
            for y in ys {
                let p = (mid, f64::from(y));
                assert_eq!(point(&mut tip, &t, line, p), Changed::No, "{edge:?} {y}");
                assert!(tip.look().is_some(), "{edge:?}: {y}");
            }
            let in_box = f64::from(row_y(look.lines[0].row) + 2);
            let box_left = f64::from(n(area.left.index()) * 16);
            point(&mut tip, &t, line, (box_left + 1.0, in_box));
            assert!(tip.look().is_some(), "{edge:?}: the box's left");
            let past = point(&mut tip, &t, line, (box_left - 1.0, in_box));
            assert_eq!(past, Changed::Yes);
            assert!(tip.look().is_none(), "{edge:?}: past the box's left");
            // A key, on the light: gone, and the next light's own.
            point(&mut tip, &t, line, (mid, on));
            assert_eq!(tip.dismiss(&t), Changed::Yes);
            assert!(tip.look().is_none());
            let still = point(&mut tip, &t, line, (mid + 1.0, on));
            assert_eq!(still, Changed::No, "stays dismissed");
            let (sx, _) = line.places.sd.unwrap();
            point(&mut tip, &t, line, (f64::from(sx.value() + 20), on));
            assert_eq!(text(tip.look().expect("SD's")), Led::Sd.tip());
            for _ in 0..100 {
                assert_eq!(tip.refresh(&t), Changed::No, "no frame takes it away");
            }
            // A narrow window leaves SD out: gone, and not back until the
            // pointer moves.
            tip.set_lights(Some(LightsLine {
                places: ByLed {
                    sd: None,
                    ..line.places
                },
                ..line
            }));
            assert_eq!(tip.refresh(&t), Changed::Yes);
            assert!(tip.look().is_none(), "{edge:?}: SD left out");
            tip.set_lights(Some(line));
            assert_eq!(tip.refresh(&t), Changed::No, "{edge:?}: stays down");
            point(&mut tip, &t, line, (f64::from(sx.value() + 21), on));
            assert!(tip.look().is_some(), "{edge:?}: the pointer moved");
        }
    }

    /// **A light's tip holds across DEC's line**: with DEC's indicator line
    /// shown and a word on it in every column it can hold, the walk up from
    /// each light to its box keeps that light's tip at every pixel row and
    /// never swaps it for a word's. Without the guard in `settle`, SD's walk
    /// turns into CS's tip and CS's into FAX's at DEC's row.
    #[test]
    fn a_lights_tip_holds_across_the_words_of_decs_line() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[1$~");
        t.set_indicator_lamps(Some(IndicatorLamps {
            cd: ReadLamp::Unknown,
            oh: ReadLamp::Unknown,
            cs: LampState::Lit,
            fax: LampState::Lit,
            syn: LampState::Lit,
        }));
        let line = line_at(LineEdge::Bottom);
        let n = |i: usize| u32::try_from(i).unwrap();
        let row_y = |r: Row| line.origin.1.value() + n(r.index()) * 34;
        for led in Led::ALL {
            let mut tip = LineTip::default();
            tip.set_lights(Some(line));
            let (x, w) = (*line.places.get(led)).expect("every light placed");
            let mid = f64::from(x.value() + w.value() / 2);
            let on = f64::from(line.top.value() + 15);
            point(&mut tip, &t, line, (mid, on));
            let look = tip.look().expect("the light's tip").clone();
            assert_eq!(text(&look), led.tip(), "{led:?}");
            for y in (row_y(look.area.top)..line.top.value() + 15).rev() {
                let changed = point(&mut tip, &t, line, (mid, f64::from(y)));
                assert_eq!(changed, Changed::No, "{led:?} at {y}");
                assert_eq!(
                    tip.look().map(text).as_deref(),
                    Some(led.tip()),
                    "{led:?} at {y}"
                );
            }
        }
    }
}
