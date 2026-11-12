//! **The state lights on DEC's indicator line**, `[window]
//! status_line_lights = "states"`: while a program shows the line (DECSSDT
//! 1), the five lights that show a lasting condition, CD, OH, CS, FAX and
//! SYN, are also words in its free columns, 62 to 79 (`Term::set_indicator_lamps`),
//! each following its condition as DEC's indicators do. "The interval
//! between the change in the state of a feature and the update of the
//! appropriate indicator shall not exceed 80 milliseconds. Indicator
//! changes are not queued: when an indicator is updated on the Status
//! Display, it shall reflect the state of that feature at the time of the
//! update" (`[[source: dec-std-070]]` 070-14 p. 14-6). So the words are
//! read fresh at each step a driver takes, the macOS display link's tick
//! and the Linux loop's turn after its reads and writes, with none of the
//! lights' hold or dwell; the core draws a change at once.
//!
//! **What each word reads**: CD and OH the driver's readings of the job in
//! front, a refused read unknown, `CD?`. On Linux OH is the terminal's
//! foreground group, the read the lights' own line makes
//! (`own_line::FrontRead`), and CD has no reader there and shows unknown.
//! CS while the line is clear, no byte owed to the program that the kernel
//! has not taken, as the lights' CS is lit.
//! FAX while a picture is placed, SYN while synchronized output is held:
//! the core's own.
//!
//! **WCAG 2.3.1 holds at every size**: the words pass by its area clause
//! while their cells are within a quarter of the field
//! (`baud_render::flash_clause`), to 37 points, and are left out past it,
//! so they never flash past the threshold and are never late
//! (`baud_render::line_lamps`).

use baud_core::{IndicatorLamps, LampState, ReadLamp, StatusType, Term};
use baud_domain::HostTime;
use baud_render::{Activity, FlashClause, Led, StatusLineLights};
use core::time::Duration;

/// "The interval between the change in the state of a feature and the
/// update of the appropriate indicator shall not exceed 80 milliseconds"
/// (`[[source: dec-std-070]]` 070-14 p. 14-6).
pub(crate) const BOUND: Duration = Duration::from_millis(80);

/// Whether DEC's line shows the words at a step: what the macOS driver
/// reads OH and CD for (`front::Reads`), and what the Linux driver reads
/// OH for and bounds its wait by (`own_line::FrontRead`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shown {
    /// The setting is off, no indicator is shown, or the cells are past
    /// the area clause.
    None,
    Words,
}

/// What a step owes the window after giving the line its words.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Owes {
    Nothing,
    /// The line's words changed while it is shown.
    Frame,
}

/// **The clause the words meet at the cell drawn**: `cell` in device
/// pixels at `scale` pixels a point, the size and zoom in force.
pub(crate) fn clause(cell: (f64, f64), scale: f64) -> FlashClause {
    let pt = |px: f64| (px / scale.max(f64::MIN_POSITIVE)) as f32;
    baud_render::flash_clause(pt(cell.0), pt(cell.1))
}

/// **Whether the line shows the words now**: asked for, the indicator
/// shown, and the cells within the area clause.
pub(crate) fn shown(setting: StatusLineLights, term: &Term<'_>, clause: FlashClause) -> Shown {
    match (setting, term.status_type(), clause) {
        (StatusLineLights::States, StatusType::Indicator, FlashClause::Area) => Shown::Words,
        _ => Shown::None,
    }
}

fn state(lit: impl FnOnce() -> bool) -> LampState {
    match lit() {
        true => LampState::Lit,
        false => LampState::Dark,
    }
}

/// **The line is clear**: `outbox` holds no byte the kernel has not taken.
/// The one reading of what is owed: DEC's line takes it as [`read`] does, and
/// the lights, the title bar's panel among them, through [`feed_cs`]. The
/// lights' CS also goes dark on a write the kernel refused any of (`Wrote`),
/// and in the film on its count of short writes.
pub(crate) fn clear(outbox: &crate::pty::Outbox) -> bool {
    outbox.is_empty()
}

/// **CS's light held while the line is not [`clear`]**: dark while bytes
/// are owed, a state, and so lit at rest ([`Led::at_rest`]).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn cs_level(outbox: &crate::pty::Outbox) -> baud_render::Level {
    match clear(outbox) {
        true => baud_render::Level::Free,
        false => baud_render::Level::Held,
    }
}

/// **CS's light told what is owed at a step**: dark while the line is not
/// [`clear`], and so lit at rest ([`Led::at_rest`]).
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) fn feed_cs(activity: &mut Activity, outbox: &crate::pty::Outbox, now: HostTime) {
    if !clear(outbox) {
        activity.seen(Led::Cs, now);
    }
}

/// **The five lamps as they are now**: `cd` and `oh` as the driver read
/// them, CS while `outbox` holds no byte the kernel has not taken, FAX
/// while a picture is placed, SYN while synchronized output is held.
pub(crate) fn read(
    term: &Term<'_>,
    outbox: &crate::pty::Outbox,
    cd: ReadLamp,
    oh: ReadLamp,
) -> IndicatorLamps {
    IndicatorLamps {
        cd,
        oh,
        cs: state(|| clear(outbox)),
        fax: state(|| term.images().any_placed()),
        syn: state(|| term.synchronized_output()),
    }
}

/// What the line is given: `lamps` under the setting and the clause
/// (`baud_render::line_lamps`).
pub(crate) fn given(
    setting: StatusLineLights,
    clause: FlashClause,
    lamps: IndicatorLamps,
) -> Option<IndicatorLamps> {
    baud_render::line_lamps(setting, clause, lamps)
}

/// **The line given `lamps`**: drawn at once by the core when they
/// changed, and a frame owed when they did while the indicator is shown.
pub(crate) fn give(term: &mut Term<'_>, lamps: Option<IndicatorLamps>) -> Owes {
    if term.indicator_lamps() == lamps {
        return Owes::Nothing;
    }
    term.set_indicator_lamps(lamps);
    match term.status_type() {
        StatusType::Indicator => Owes::Frame,
        StatusType::None | StatusType::HostWritable => Owes::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 6,
        ws_col: 80,
    };

    fn feed(t: &mut Term<'_>, b: &[u8]) {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }

    fn words(t: &Term<'_>) -> String {
        let line = t.status_line().expect("the indicator");
        line.cells[61..]
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

    /// **Each step gives the line the lamps as they are**: SYN while the
    /// program holds synchronized output, CS while nothing is owed, CD
    /// unknown where it cannot be read; a change owes a frame while the
    /// indicator is shown, none while it is not, and a step that changes
    /// nothing owes nothing. Off and past the area clause the line has
    /// none.
    #[test]
    fn each_step_gives_the_line_the_lamps_as_they_are() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut outbox = crate::pty::Outbox::default();
        let unknown = ReadLamp::Unknown;
        let step = |t: &mut Term<'_>, outbox: &crate::pty::Outbox, s, c| {
            let lamps = read(t, outbox, unknown, ReadLamp::default());
            give(t, given(s, c, lamps))
        };
        let (on, area) = (StatusLineLights::States, FlashClause::Area);
        assert_eq!(shown(on, &t, area), Shown::None, "no indicator yet");
        assert_eq!(step(&mut t, &outbox, on, area), Owes::Nothing, "none shown");
        feed(&mut t, b"\x1b[1$~");
        assert_eq!(shown(on, &t, area), Shown::Words);
        assert_eq!(words(&t), "CD?     CS");
        feed(&mut t, b"\x1b[?2026h");
        assert_eq!(step(&mut t, &outbox, on, area), Owes::Frame);
        assert_eq!(words(&t), "CD?     CS     SYN");
        assert_eq!(step(&mut t, &outbox, on, area), Owes::Nothing);
        outbox.push(b"x");
        assert_eq!(step(&mut t, &outbox, on, area), Owes::Frame);
        assert_eq!(
            words(&t),
            "CD?            SYN",
            "owed: the line is not clear"
        );
        feed(&mut t, b"\x1b[?2026l");
        outbox = crate::pty::Outbox::default();
        assert_eq!(step(&mut t, &outbox, on, area), Owes::Frame);
        assert_eq!(words(&t), "CD?     CS");
        assert_eq!(shown(on, &t, FlashClause::Count), Shown::None);
        assert_eq!(step(&mut t, &outbox, on, FlashClause::Count), Owes::Frame);
        assert_eq!(words(&t), "", "past the area clause, none");
        assert_eq!(step(&mut t, &outbox, on, area), Owes::Frame);
        assert_eq!(shown(StatusLineLights::Off, &t, area), Shown::None);
        assert_eq!(
            step(&mut t, &outbox, StatusLineLights::Off, area),
            Owes::Frame
        );
        assert_eq!(words(&t), "", "off, none");
    }

    /// **CS at rest, with something queued and after it, on DEC's line and
    /// in the title bar's panel**, one outbox read by both: lit with nothing
    /// owed, dark with a byte queued; emptied, the line's word is back at
    /// once and the panel's lamp after its hold, as a light is. What the
    /// panel draws for what `Activity` shows is `titlebar_mark.rs`'s.
    /// Perturbation: [`clear`] inverted, or [`feed_cs`] feeding with the
    /// line clear, and this goes red.
    #[test]
    fn cs_is_lit_at_rest_and_dark_while_a_byte_is_owed() {
        use baud_render::{Light, Pace};
        let at = |ms: u64| HostTime::ZERO.plus(Duration::from_millis(ms));
        let mut owned = Owned::new(&WS);
        let t = Term::new(owned.storage(), &WS);
        let mut lights = Activity::new();
        let (none, step) = (ReadLamp::default(), |a: &mut Activity, o, now| {
            feed_cs(a, o, now);
            a.step(now, Pace::Full).cs
        });
        let rest = crate::pty::Outbox::default();
        assert_eq!(read(&t, &rest, none, none).cs, LampState::Lit);
        assert_eq!(step(&mut lights, &rest, at(0)), Light::Lit, "at rest");
        let mut owed = crate::pty::Outbox::default();
        owed.push(b"x");
        assert_eq!(read(&t, &owed, none, none).cs, LampState::Dark);
        assert_eq!(step(&mut lights, &owed, at(8)), Light::Dark, "queued");
        assert_eq!(read(&t, &rest, none, none).cs, LampState::Lit);
        assert_eq!(step(&mut lights, &rest, at(16)), Light::Dark, "the hold");
        assert_eq!(step(&mut lights, &rest, at(1_000)), Light::Lit, "clear");
    }

    /// **Both drivers give the line its words at each step**: macOS on
    /// every turn and at the tick after its pump, which reads OH and CD for
    /// the line with the lights off too (`give_line_words`), and the Linux turn after its reads, its flush and
    /// the lights' own line, before it presents, OH read from who is in
    /// front while the words show and the wait bounded for it, CD
    /// unknown. Read off the files, as the lights' wiring is.
    #[test]
    fn both_drivers_give_the_line_its_words_at_each_step() {
        const MAC: &str = include_str!("app.rs");
        const LINUX: &str = include_str!("linux.rs");
        let body = |src: &'static str, head: &str| {
            let at = src.find(head).unwrap_or_else(|| panic!("{head}"));
            let rest = &src[at..];
            rest.split_once("\n    }\n").map_or(rest, |(b, _)| b)
        };
        let mac = body(
            MAC,
            concat!("    fn show_lights(&mut self, ", "target: f64) {"),
        );
        for want in [
            "crate::line_lamps::clause(self.app.metrics_cell(), self.app.scale)",
            "crate::line_lamps::shown(self.app.status_line_lights, &self.app.term, clause)",
            "crate::front::Reads::of(setting, line, self.app.interval)",
            "self.give_line_words(clause);",
        ] {
            assert!(mac.contains(want), "{want}");
        }
        let words = body(
            MAC,
            concat!(
                "    fn give_line_",
                "words(&mut self, clause: baud_render::FlashClause) {"
            ),
        );
        for want in [
            "crate::line_lamps::given(self.app.status_line_lights, clause, lamps)",
            "crate::line_lamps::give(&mut self.app.term, given) == crate::line_lamps::Owes::Frame",
        ] {
            assert!(words.contains(want), "{want}");
        }
        let turn = body(MAC, concat!("    fn lights_", "turn(&mut self) {"));
        assert!(turn.contains("self.give_line_words(clause);"), "every turn");
        let turn = LINUX
            .split_once("        d.flush_reply();\n")
            .map(|(_, rest)| rest)
            .expect("the turn's flush");
        assert!(
            turn.starts_with("        d.tick_lights();\n        // DEC's line, after this turn's reads and writes (`line_lamps.rs`).\n        d.show_line_lamps();\n        d.present();"),
            "between the flush and the present"
        );
        let linux = body(LINUX, concat!("    fn show_line_lamps(&mut self", ") {"));
        for want in [
            "let clause = self.line_clause();",
            "crate::line_lamps::shown(self.status_line_lights, &self.term, clause)",
            "crate::own_line::lamp(self.in_front(self.timed.clock.host(Instant::now())))",
            "let cd = baud_core::ReadLamp::Unknown;",
            "crate::line_lamps::read(&self.term, &self.outbox, cd, oh)",
            "crate::line_lamps::given(self.status_line_lights, clause, lamps)",
            "crate::line_lamps::give(&mut self.term, given) == crate::line_lamps::Owes::Frame",
        ] {
            assert!(linux.contains(want), "{want}");
        }
        let wait = body(
            LINUX,
            concat!("    fn line_lamps_wait(&self, ", "now: Instant)"),
        );
        for want in [
            "crate::line_lamps::shown(self.status_line_lights, &self.term, self.line_clause())",
            ".unwrap_or(crate::own_line::SAMPLE)",
            "self.front.line_wait(self.timed.clock.host(now), frame)",
        ] {
            assert!(wait.contains(want), "{want}");
        }
        for want in [
            "let words = d.line_lamps_wait(now);",
            "crate::own_line::in_front(master.and_then(crate::fg::group), shell)",
        ] {
            assert_eq!(LINUX.matches(want).count(), 1, "{want}");
        }
        // The words' deadline among the bounds of the loop's one wait, read
        // by its items, so a list rustfmt lays out again still counts.
        assert!(
            crate::own_line::wait_bounds(LINUX)
                .iter()
                .any(|b| b == "words"),
            "the words bound the wait"
        );
    }

    /// **The clause is read at the cell in points**: 8 x 17 device pixels
    /// at a scale of one are 13 points of JetBrains Mono, and the same
    /// cell at 2x is half the points; a cell five times as large is past
    /// the area clause.
    #[test]
    fn the_clause_is_read_at_the_cell_in_points() {
        assert_eq!(clause((8.0, 17.0), 1.0), FlashClause::Area);
        assert_eq!(clause((16.0, 34.0), 2.0), FlashClause::Area);
        assert_eq!(clause((40.0, 85.0), 1.0), FlashClause::Count);
        assert_eq!(clause((40.0, 85.0), 2.0), FlashClause::Area);
    }
}
