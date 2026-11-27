//! **The copy signal, for both drivers**: what a copy says and for how long,
//! pure, so its tests run on every platform.
//!
//! **The words**: "Copied N characters", "1 character" for one, for a copy
//! of the selection; "A program copied N characters" for a program's `OSC
//! 52`, never its text, and always in the bar, since it has no selection to
//! be placed by. On Linux a copy of the selection names the selections it
//! was set on, "to the clipboard", "to the primary selection" or both,
//! since SELECT can name either or both (`clip.rs`); on macOS the one
//! pasteboard is named by neither. A
//! refused `OSC 52` says nothing here and stays the log line it is
//! (`clip.rs`). A character here is a Unicode scalar value, as Rust's `chars`
//! counts them.
//!
//! **It ends on a deadline**, the frame's clock reaching [`MESSAGE_FOR`] after
//! the first frame that drew it, as the bell's flash does (`bell.rs`), or at
//! any key, which still goes to the program. Nothing sleeps, no thread blocks
//! and no frame is held: it costs a frame to start and a frame to end. **The
//! end is never lost**: a frame whose present fails is dropped on macOS, and
//! one that changed what the signal shows is owed again [`RETRY`] later, not
//! on every tick while presents keep failing, the defect a tick's retry had
//! in the bell (`Bell::refused`).
//!
//! **A program cannot use it to flood anything.** A read batch's copies are
//! one ([`Batch`]). While words are up a program's copy changes nothing at
//! once: the last one is held and shown as the words up now end, so a program
//! looping `OSC 52` costs one frame each [`MESSAGE_FOR`], and the bar says
//! the last count it copied. Its announcements are held to the same pace.
//!
//! **The link preview keeps the bar.** Under it no words are drawn, and none
//! wait for it to end: [`Preview::Up`] takes the words up and the one held.
//! The renderer draws a tip over the bar, so words drawn then would cover
//! the address the preview is there to show.
//!
//! **It is an overlay and nothing else**: the words reach the renderer's tip
//! and the pulse its selection's colours, and nothing here writes a cell, so
//! no query of the core's can read either back.
//!
//! **The pulse**, when `[message.copy] pulse` asks for one: the selection in
//! the message pair for [`PULSE_FOR`], for a copy of the selection only. It
//! does not run under Reduce Motion or Dim Flashing Lights
//! ([`crate::access::pulse_may_run`]), with both colours `"none"`, which
//! leaves it nothing of its own to flash, or where the bell's budget has no
//! room ([`crate::bell::Bell::pulse`]).

use core::time::Duration;

use baud_domain::{Cols, Rows};
use baud_render::{
    Highlight, MESSAGE_FOR, MessageAt, SelectionPulse, TipAnchor, TipLook, message_look,
};

use crate::access::FlashingLights;
use crate::bell::Moment;
use crate::config::{CopyPulse, CopySignal, Message, MessagePlace, SystemMotion};

/// How long the selection takes the message pair.
pub(crate) const PULSE_FOR: Duration = Duration::from_millis(150);

/// How soon a frame the signal changed and whose present failed is owed again.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub(crate) const RETRY: Duration = Duration::from_millis(100);

/// Who copied, or what a click on a printed path came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Source {
    /// A copy of the selection, and the selections it was set on where the
    /// system has more than one.
    Selection(Option<CopiedTo>),
    Program,
    Path(PathNote),
    /// A copy the clipboard did not keep: on Linux, no selection asked
    /// took it.
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "linux", test)),
        allow(dead_code)
    )]
    NotCopied,
}

/// **The selections a copy was set on**, from the ones that took it. Made
/// only where there is more than one selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) enum CopiedTo {
    Clipboard,
    Primary,
    Both,
}

impl CopiedTo {
    /// The selections in `took`, the ones that took the copy; `None` when
    /// it names neither.
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn of(took: &[crate::clip::Board]) -> Option<CopiedTo> {
        use crate::clip::Board;
        match (
            took.contains(&Board::Clipboard),
            took.contains(&Board::Primary),
        ) {
            (true, true) => Some(CopiedTo::Both),
            (true, false) => Some(CopiedTo::Clipboard),
            (false, true) => Some(CopiedTo::Primary),
            (false, false) => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            CopiedTo::Clipboard => "the clipboard",
            CopiedTo::Primary => "the primary selection",
            CopiedTo::Both => "the clipboard and the primary selection",
        }
    }
}

/// Whether the window had the keyboard when a copy was asked of a
/// selection.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Keyboard {
    Here,
    Away,
}

/// What a selection answered a copy asked of it.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Took {
    /// The selection took it: the compositor asked its source for the text.
    Owned,
    /// It was cancelled first, and the selection kept what it had.
    Cancelled,
}

/// One selection's part in a copy asked of the compositor.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Answer {
    Unasked,
    Waiting(Keyboard),
    Answered(Took),
}

/// **A copy of the selection the compositor has yet to answer**: the
/// characters it holds and each selection's answer so far. It says nothing
/// until every selection asked has answered ([`Asked::verdict`]).
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Asked {
    count: Characters,
    clipboard: Answer,
    primary: Answer,
}

/// **What a copy says once every selection asked has answered.**
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Verdict {
    /// One or more took it: the words name those.
    Copied(Characters, CopiedTo),
    /// None did: "Not copied: the clipboard kept what it had".
    NotCopied,
}

#[cfg(any(target_os = "linux", test))]
impl Asked {
    /// A copy of `count` characters, asked of no selection yet.
    pub(crate) fn new(count: Characters) -> Asked {
        Asked {
            count,
            clipboard: Answer::Unasked,
            primary: Answer::Unasked,
        }
    }

    fn part(&mut self, b: crate::clip::Board) -> &mut Answer {
        match b {
            crate::clip::Board::Clipboard => &mut self.clipboard,
            crate::clip::Board::Primary => &mut self.primary,
        }
    }

    /// The copy asked of `b`, with the keyboard or without it.
    pub(crate) fn ask(&mut self, b: crate::clip::Board, keyboard: Keyboard) {
        *self.part(b) = Answer::Waiting(keyboard);
    }

    /// The copy, if any selection was asked.
    pub(crate) fn pending(self) -> Option<Asked> {
        match (self.clipboard, self.primary) {
            (Answer::Unasked, Answer::Unasked) => None,
            _ => Some(self),
        }
    }

    /// **`b` answered**, and how the copy was asked of it; `None` for a
    /// selection it was not asked of or that answered already, which
    /// changes nothing.
    pub(crate) fn answer(&mut self, b: crate::clip::Board, took: Took) -> Option<Keyboard> {
        let part = self.part(b);
        let Answer::Waiting(keyboard) = *part else {
            return None;
        };
        *part = Answer::Answered(took);
        Some(keyboard)
    }

    /// What the copy says, once no selection asked is waiting.
    pub(crate) fn verdict(&self) -> Option<Verdict> {
        use crate::clip::Board;
        let parts = [
            (Board::Clipboard, self.clipboard),
            (Board::Primary, self.primary),
        ];
        if parts.iter().any(|(_, a)| matches!(a, Answer::Waiting(_))) {
            return None;
        }
        let owned = parts
            .iter()
            .filter(|(_, a)| *a == Answer::Answered(Took::Owned))
            .map(|(b, _)| *b)
            .collect::<Vec<Board>>();
        Some(match CopiedTo::of(&owned) {
            Some(to) => Verdict::Copied(self.count, to),
            None => Verdict::NotCopied,
        })
    }
}

/// **Why a click on a printed path or a struck link opened nothing**, said
/// in the bar where the copy's words go: the worker asked again and found it
/// gone, could not ask, or the path is too long for a link (`paths.rs`); or
/// the system had no app for the link's kind when the click tried it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PathNote {
    Missing,
    Unchecked,
    TooLong,
    /// Said only on macOS, the one system asked whether a kind has an app.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    NoApp(crate::link::Scheme),
}

/// **The words for a copy the clipboard did not keep.**
pub(crate) const NOT_COPIED: &str = "Not copied: the clipboard kept what it had";

/// **The words a copy says.**
pub(crate) fn words(source: Source, count: Characters) -> String {
    let noun = if count == Characters(1) {
        "character"
    } else {
        "characters"
    };
    match source {
        Source::Selection(None) => format!("Copied {count} {noun}"),
        Source::Selection(Some(to)) => format!("Copied {count} {noun} to {}", to.name()),
        Source::Program => format!("A program copied {count} {noun}"),
        Source::Path(PathNote::Missing) => format!("No such file on {}", crate::paths::MACHINE),
        Source::Path(PathNote::Unchecked) => {
            format!("Couldn't check this file on {}", crate::paths::MACHINE)
        }
        Source::Path(PathNote::TooLong) => "This path is too long to open".to_string(),
        Source::NotCopied => NOT_COPIED.to_string(),
        Source::Path(PathNote::NoApp(s)) => match scheme_name(s) {
            Some(name) => format!("No app for {name}:"),
            None => "No app for this kind of link".to_string(),
        },
    }
}

/// A kind of link's scheme as a link spells it: none for one a rule of
/// yours added, which has no one name.
fn scheme_name(s: crate::link::Scheme) -> Option<&'static str> {
    use crate::link::Scheme;
    match s {
        Scheme::Http => Some("http"),
        Scheme::Https => Some("https"),
        Scheme::Mailto => Some("mailto"),
        Scheme::File => Some("file"),
        Scheme::Added => None,
    }
}

/// **How many characters a copy's text holds**: Unicode scalar values, not
/// bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Characters(pub(crate) usize);

impl core::fmt::Display for Characters {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

/// How many characters a copy's text holds.
pub(crate) fn characters(text: &str) -> Characters {
    Characters(text.chars().count())
}

/// Whether a frame is owed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Owes {
    Nothing,
    Frame,
}

/// Whether the link preview is on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Preview {
    Up,
    None,
}

impl Preview {
    /// From what the driver hands the renderer as the preview.
    pub(crate) fn of<L>(look: Option<&L>) -> Preview {
        match look {
            Some(_) => Preview::Up,
            None => Preview::None,
        }
    }
}

/// **A read batch's copies by a program**: filled after each read, whose
/// write the core keeps only until the next, and said once after the batch.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Batch(Option<Characters>);

impl Batch {
    /// A program's copy of `text` reached the clipboard: the last one counts.
    pub(crate) fn copied(&mut self, text: &str) {
        self.0 = Some(characters(text));
    }
}

/// **What a copy of the selection decides of the pulse**, from the settings;
/// the bell's budget is asked after this.
pub(crate) fn pulse_runs(message: Message, motion: SystemMotion, lights: FlashingLights) -> bool {
    message.copy.enable == CopySignal::On
        && message.copy.pulse == CopyPulse::On
        && message.colours.has_its_own()
        && crate::access::pulse_may_run(motion, lights)
}

/// Whether a copy of the selection pulses, the budget having been asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Pulse {
    Run,
    Skip,
}

/// Where words go, once a frame has read where the selection is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Place {
    /// By the selection the next frame finds.
    Selection,
    At(MessageAt),
}

/// The words up, or owed the frame that draws them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Up<T> {
    source: Source,
    count: Characters,
    place: Place,
    /// When they end: from the first frame that drew them.
    until: Option<T>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pulsing<T> {
    Off,
    /// The next frame starts it.
    Owed,
    Until(T),
}

/// **What a frame shows of the signal.**
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Frame {
    pub(crate) look: Option<TipLook>,
    pub(crate) pulse: SelectionPulse,
    /// Whether this differs from what the last frame was told, or a dropped
    /// frame's retry has come round.
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    pub(crate) owes: Owes,
}

/// **The signal's state**, on a clock the bell's frames share.
pub(crate) struct Signal<T> {
    message: Message,
    up: Option<Up<T>>,
    /// A program's copy, shown as the words up end.
    held: Option<Characters>,
    pulse: Pulsing<T>,
    /// When a program's copy may next be spoken.
    speak_from: Option<T>,
    /// The words the driver has yet to hand a screen reader.
    saying: Option<String>,
    /// What the last frame was told, and whether a frame presented since.
    shown: (Option<TipLook>, SelectionPulse),
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    unpresented: Owes,
    retry: Option<T>,
}

impl<T: Moment> Signal<T> {
    pub(crate) fn new(message: Message) -> Signal<T> {
        Signal {
            message,
            up: None,
            held: None,
            pulse: Pulsing::Off,
            speak_from: None,
            saying: None,
            shown: (None, SelectionPulse::Off),
            unpresented: Owes::Nothing,
            retry: None,
        }
    }

    /// **The settings read again**: colours are the renderer's and take
    /// effect at the next frame; off takes down what is up.
    pub(crate) fn set(&mut self, message: Message) {
        self.message = message;
        if message.copy.enable == CopySignal::Off {
            self.up = None;
            self.held = None;
            self.pulse = Pulsing::Off;
        }
    }

    /// The settings in force.
    pub(crate) fn message(&self) -> Message {
        self.message
    }

    /// **The user copied the selection**, to the system's one pasteboard.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn selection(&mut self, count: Characters, pulse: Pulse) -> Owes {
        self.selection_to(None, count, pulse)
    }

    /// **The user copied the selection, to the selections `to` names.** Its
    /// words replace any up, a person being who the last word is owed to,
    /// and are spoken at once.
    pub(crate) fn selection_to(
        &mut self,
        to: Option<CopiedTo>,
        count: Characters,
        pulse: Pulse,
    ) -> Owes {
        if self.message.copy.enable == CopySignal::Off || count == Characters(0) {
            return Owes::Nothing;
        }
        self.up = Some(Up {
            source: Source::Selection(to),
            count,
            place: match self.message.copy.place {
                MessagePlace::Tip => Place::Selection,
                MessagePlace::Bar => Place::At(MessageAt::Bar),
            },
            until: None,
        });
        if pulse == Pulse::Run {
            self.pulse = Pulsing::Owed;
        }
        self.saying = Some(words(Source::Selection(to), count));
        Owes::Frame
    }

    /// **A read batch's copies by a program**, after the batch: words in the
    /// bar, or held while words are up, which owes no frame.
    pub(crate) fn program(&mut self, batch: Batch, now: T) -> Owes {
        let Batch(Some(count)) = batch else {
            return Owes::Nothing;
        };
        if self.message.copy.enable == CopySignal::Off || count == Characters(0) {
            return Owes::Nothing;
        }
        if self.up.is_some() {
            self.held = Some(count);
            return Owes::Nothing;
        }
        self.raise_program(count, now);
        Owes::Frame
    }

    fn raise_program(&mut self, count: Characters, now: T) {
        self.up = Some(Up {
            source: Source::Program,
            count,
            place: Place::At(MessageAt::Bar),
            until: None,
        });
        self.speak_program(count, now);
    }

    /// A program's copy spoken, at most once each [`MESSAGE_FOR`].
    fn speak_program(&mut self, count: Characters, now: T) {
        if self.speak_from.is_some_and(|from| now < from) {
            return;
        }
        self.speak_from = Some(now.after(MESSAGE_FOR));
        self.saying = Some(words(Source::Program, count));
    }

    /// **A copy the clipboard did not keep**: said in the bar, as a
    /// program's copy is, whatever `[message.copy]` says, since nothing was
    /// copied.
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "linux", test)),
        allow(dead_code)
    )]
    pub(crate) fn not_copied(&mut self) -> Owes {
        self.up = Some(Up {
            source: Source::NotCopied,
            count: Characters(0),
            place: Place::At(MessageAt::Bar),
            until: None,
        });
        self.held = None;
        self.saying = Some(NOT_COPIED.to_string());
        Owes::Frame
    }

    /// **A click on a printed path opened nothing**: why, in the bar, as a
    /// program's copy is said, whatever `[message.copy]` says, since it is
    /// no copy.
    pub(crate) fn path(&mut self, note: PathNote) -> Owes {
        self.up = Some(Up {
            source: Source::Path(note),
            count: Characters(0),
            place: Place::At(MessageAt::Bar),
            until: None,
        });
        self.held = None;
        self.saying = Some(words(Source::Path(note), Characters(0)));
        Owes::Frame
    }

    /// **A key was typed**: the words go, and the key goes on to the program.
    /// A pulse runs out on its own, being over in a moment.
    pub(crate) fn dismiss(&mut self) -> Owes {
        self.held = None;
        match self.up.take() {
            Some(_) => Owes::Frame,
            None => Owes::Nothing,
        }
    }

    /// **The frame seen at `now`**: what it shows, each deadline it reaches
    /// ended, and the clock of whatever it starts begun. Asked before a frame
    /// is built, on macOS on every tick; with nothing up it does no work.
    pub(crate) fn frame(
        &mut self,
        now: T,
        preview: Preview,
        highlight: Option<&Highlight>,
        cols: Cols,
        rows: Rows,
    ) -> Frame {
        // A retry that has come round owes this frame; one still ahead stands,
        // this being asked on every tick.
        let retried = self.retry.is_some_and(|r| now >= r);
        if retried {
            self.retry = None;
        }
        self.pulse = match self.pulse {
            Pulsing::Off => Pulsing::Off,
            // Nothing on the view to flash: a Cmd-C of text kept after its
            // highlight went.
            Pulsing::Owed if highlight.is_none() => Pulsing::Off,
            Pulsing::Owed => Pulsing::Until(now.after(PULSE_FOR)),
            Pulsing::Until(end) if now >= end => Pulsing::Off,
            Pulsing::Until(end) => Pulsing::Until(end),
        };
        let pulse = match self.pulse {
            Pulsing::Off => SelectionPulse::Off,
            Pulsing::Owed | Pulsing::Until(_) => SelectionPulse::On,
        };
        if preview == Preview::Up {
            self.up = None;
            if let Some(count) = self.held.take() {
                self.speak_program(count, now);
            }
        }
        if self
            .up
            .and_then(|u| u.until)
            .is_some_and(|until| now >= until)
        {
            self.up = None;
            if let Some(count) = self.held.take() {
                self.raise_program(count, now);
            }
        }
        let look = self.up.as_mut().and_then(|u| {
            if u.until.is_none() {
                u.until = Some(now.after(MESSAGE_FOR));
            }
            let at = match u.place {
                Place::At(at) => at,
                Place::Selection => {
                    let anchor = highlight.and_then(TipAnchor::of);
                    anchor.map_or(MessageAt::Bar, MessageAt::Tip)
                }
            };
            u.place = Place::At(at);
            message_look(&words(u.source, u.count), cols, rows, at)
        });
        let changed = self.shown.0 != look || self.shown.1 != pulse;
        if changed {
            self.shown = (look.clone(), pulse);
            self.unpresented = Owes::Frame;
        }
        Frame {
            look,
            pulse,
            owes: match changed || retried {
                true => Owes::Frame,
                false => Owes::Nothing,
            },
        }
    }

    /// The frame reached the display: nothing it changed is owed again.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn presented(&mut self) {
        self.unpresented = Owes::Nothing;
    }

    /// **The frame's present failed, so it was dropped**: one that changed
    /// what the signal shows is owed again, so neither its start nor its end
    /// is lost, at a pace that does not force a frame a tick while presents
    /// keep failing.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn refused(&mut self, now: T) {
        if self.unpresented == Owes::Frame {
            self.retry = Some(now.after(RETRY));
        }
    }

    /// **When the next frame the signal owes is due**, for a loop that waits
    /// on a clock.
    pub(crate) fn deadline(&self) -> Option<T> {
        let words = self.up.and_then(|u| u.until);
        let pulse = match self.pulse {
            Pulsing::Until(end) => Some(end),
            Pulsing::Off | Pulsing::Owed => None,
        };
        [words, pulse, self.retry].into_iter().flatten().min()
    }

    /// Whether that frame is owed at `now`.
    pub(crate) fn is_due(&self, now: T) -> bool {
        self.deadline().is_some_and(|d| now >= d)
    }

    /// **The words a screen reader is owed**, once each.
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    pub(crate) fn take_saying(&mut self) -> Option<String> {
        self.saying.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_domain::{Col, Row};
    use baud_render::{Hints, MessageColours, MessagePaint, Region};
    use std::time::Instant;

    fn cols() -> Cols {
        Cols::of_index(80)
    }

    fn rows() -> Rows {
        Rows::of_index(24)
    }

    const MS: Duration = Duration::from_millis(1);

    fn zero() -> Instant {
        Instant::now()
    }

    fn selected() -> Highlight {
        Highlight::new(
            (Row::from_index(3), Col::at_index(5)),
            (Row::from_index(3), Col::at_index(9)),
            Region::Stream,
        )
    }

    fn signal(message: Message) -> Signal<Instant> {
        Signal::new(message)
    }

    /// The frame at `t`, a selection on the view and no preview.
    fn drawn(s: &mut Signal<Instant>, t: Instant) -> Frame {
        s.frame(t, Preview::None, Some(&selected()), cols(), rows())
    }

    fn said(look: &TipLook) -> String {
        let text = &look.lines[0].text;
        text.iter()
            .filter_map(|c| char::from_u32(c.value()))
            .collect()
    }

    fn batch(text: &str) -> Batch {
        let mut b = Batch::default();
        b.copied(text);
        b
    }

    /// **A copy says what the selections answered, once all have**: asked
    /// of both, nothing is said while either waits; one owned and one
    /// cancelled names the one that took it; neither owned says the
    /// clipboard kept what it had, in the copy's place. A second answer,
    /// or one from a selection not asked, changes nothing. Perturbation:
    /// say the copy on the first answer.
    #[test]
    fn a_copy_says_what_the_selections_answered() {
        use crate::clip::Board;
        let both = || {
            let mut a = Asked::new(Characters(9));
            a.ask(Board::Clipboard, Keyboard::Here);
            a.ask(Board::Primary, Keyboard::Away);
            a.pending().expect("asked")
        };
        let mut a = both();
        assert_eq!(a.verdict(), None, "both wait");
        assert_eq!(
            a.answer(Board::Clipboard, Took::Owned),
            Some(Keyboard::Here)
        );
        assert_eq!(a.verdict(), None, "the primary waits");
        assert_eq!(
            a.answer(Board::Primary, Took::Cancelled),
            Some(Keyboard::Away)
        );
        assert_eq!(
            a.verdict(),
            Some(Verdict::Copied(Characters(9), CopiedTo::Clipboard))
        );
        assert_eq!(a.answer(Board::Primary, Took::Owned), None);
        let mut s = signal(Message::default());
        assert_eq!(
            s.selection_to(Some(CopiedTo::Clipboard), Characters(9), Pulse::Skip),
            Owes::Frame
        );
        assert_eq!(
            s.take_saying().as_deref(),
            Some("Copied 9 characters to the clipboard")
        );
        let mut a = both();
        a.answer(Board::Clipboard, Took::Cancelled);
        a.answer(Board::Primary, Took::Cancelled);
        assert_eq!(a.verdict(), Some(Verdict::NotCopied));
        assert_eq!(s.not_copied(), Owes::Frame);
        assert_eq!(
            s.take_saying().as_deref(),
            Some("Not copied: the clipboard kept what it had")
        );
        let f = drawn(&mut s, zero());
        assert_eq!(
            said(&f.look.expect("shown")).trim(),
            "Not copied: the clipboard kept what it had"
        );
        let mut one = Asked::new(Characters(9));
        one.ask(Board::Primary, Keyboard::Here);
        assert_eq!(one.answer(Board::Clipboard, Took::Owned), None, "not asked");
        assert_eq!(Asked::new(Characters(9)).pending(), None);
    }

    /// **The words, each shape**: one is singular; a copy of the selection
    /// on the one pasteboard names no selection; a program's is told apart,
    /// names no selection and carries no text of its. Perturbations: drop
    /// the singular arm; spell "to clipboard" in the pasteboard's format.
    #[test]
    fn the_words_are_the_counts() {
        assert_eq!(
            words(Source::Selection(None), Characters(1)),
            "Copied 1 character"
        );
        assert_eq!(
            words(Source::Selection(None), Characters(42)),
            "Copied 42 characters"
        );
        assert_eq!(
            words(Source::Selection(None), Characters(0)),
            "Copied 0 characters"
        );
        assert_eq!(
            words(Source::Program, Characters(1)),
            "A program copied 1 character"
        );
        assert_eq!(
            words(Source::Program, Characters(7)),
            "A program copied 7 characters"
        );
        for s in [Source::Selection(None), Source::Program] {
            for n in [0, 1, 2] {
                let w = words(s, Characters(n)).to_lowercase();
                assert!(!w.contains("clipboard") && !w.contains("primary"), "{w}");
            }
        }
        assert_eq!(
            characters("h\u{e9}llo"),
            Characters(5),
            "scalars, not bytes"
        );
    }

    /// **A copy of the selection to named selections says them**, and so do
    /// the words it speaks; the selections are the ones that took it, `None`
    /// when neither did. Perturbation: words that ignore `to`.
    #[test]
    fn the_words_name_the_selections_a_copy_was_set_on() {
        for (to, said) in [
            (CopiedTo::Clipboard, "Copied 1 character to the clipboard"),
            (
                CopiedTo::Primary,
                "Copied 1 character to the primary selection",
            ),
            (
                CopiedTo::Both,
                "Copied 1 character to the clipboard and the primary selection",
            ),
        ] {
            assert_eq!(words(Source::Selection(Some(to)), Characters(1)), said);
        }
        use crate::clip::Board::{Clipboard, Primary};
        assert_eq!(CopiedTo::of(&[Primary, Clipboard]), Some(CopiedTo::Both));
        assert_eq!(CopiedTo::of(&[Clipboard]), Some(CopiedTo::Clipboard));
        assert_eq!(CopiedTo::of(&[Primary]), Some(CopiedTo::Primary));
        assert_eq!(CopiedTo::of(&[]), None, "nothing took it");
        let mut s = signal(Message::default());
        assert_eq!(
            s.selection_to(Some(CopiedTo::Primary), Characters(3), Pulse::Skip),
            Owes::Frame
        );
        assert_eq!(
            s.take_saying().as_deref(),
            Some("Copied 3 characters to the primary selection")
        );
    }

    /// **A click on a printed path that opened nothing says why in the bar,
    /// with the copy's words off too**, naming this machine and no path.
    /// Perturbation: gate `path` on `[message.copy] enable`.
    #[test]
    fn a_path_click_that_opens_nothing_says_why() {
        let mut m = Message::default();
        m.copy.enable = CopySignal::Off;
        let mut s = signal(m);
        assert_eq!(s.path(PathNote::Missing), Owes::Frame);
        let f = drawn(&mut s, zero());
        let look = f.look.expect("the words");
        let w = said(&look);
        assert!(w.contains("No such file on this"), "{w}");
        assert!(!w.contains('/'), "no path: {w}");
        assert_eq!(s.take_saying().as_deref(), Some(w.trim()));
    }

    /// **Up for 1.5 s from its first drawn frame, one frame to start and one
    /// to end, then nothing owed.** Perturbations: stamp `until` at the copy,
    /// not the frame; drop `is_due`'s words from `deadline`, and the end
    /// never comes.
    #[test]
    fn words_end_on_their_deadline_and_owe_two_frames() {
        let t0 = zero();
        let mut s = signal(Message::default());
        assert_eq!(s.selection(Characters(42), Pulse::Skip), Owes::Frame);
        assert_eq!(s.deadline(), None, "no clock before a frame draws it");
        let start = t0 + 200 * MS;
        let f = drawn(&mut s, start);
        assert_eq!(f.owes, Owes::Frame, "the start");
        assert_eq!(
            said(f.look.as_ref().unwrap()).trim(),
            "Copied 42 characters"
        );
        assert_eq!(s.deadline(), Some(start + MESSAGE_FOR));
        assert!(!s.is_due(start + MESSAGE_FOR - MS));
        assert!(s.is_due(start + MESSAGE_FOR));
        let held = drawn(&mut s, start + 100 * MS);
        assert_eq!(held.owes, Owes::Nothing, "a tick in between owes nothing");
        assert!(held.look.is_some());
        let end = drawn(&mut s, start + MESSAGE_FOR);
        assert_eq!(end.owes, Owes::Frame, "the end");
        assert_eq!(end.look, None);
        assert_eq!(s.deadline(), None, "nothing more is owed");
        assert!(!s.is_due(start + MESSAGE_FOR * 10));
        assert_eq!(drawn(&mut s, start + MESSAGE_FOR * 10).owes, Owes::Nothing);
    }

    /// **The tip is placed by the selection, once**, and a selection gone by
    /// the frame after does not move it; with none on the view the words go
    /// in the bar. Perturbation: read the highlight on every frame.
    #[test]
    fn a_tip_is_placed_by_the_selection_of_its_first_frame() {
        let t = zero();
        let mut s = signal(Message::default());
        s.selection(Characters(5), Pulse::Skip);
        let look = drawn(&mut s, t).look.unwrap();
        assert_eq!(look.lines[0].row, Row::from_index(4), "under it");
        assert_eq!(look.lines[0].col, Col::at_index(5), "from its left");
        let f = s.frame(t + MS, Preview::None, None, cols(), rows());
        assert_eq!(f.look.unwrap().lines[0].row, Row::from_index(4));
        s.selection(Characters(5), Pulse::Skip);
        let f = s.frame(t + 2 * MS, Preview::None, None, cols(), rows());
        assert_eq!(f.look.unwrap().lines[0].row, Row::from_index(23), "the bar");
    }

    /// **Each look is in the message pair**, so every draw path reads it so.
    /// Perturbation: build it with `Hints::SELECTION` in `message_look`.
    #[test]
    fn the_words_are_drawn_in_the_pair() {
        let mut s = signal(Message::default());
        s.selection(Characters(5), Pulse::Skip);
        let look = drawn(&mut s, zero()).look.unwrap();
        assert!(
            look.lines
                .iter()
                .flat_map(|l| &l.hints)
                .all(|h| *h == Hints::MESSAGE)
        );
    }

    /// **`place = "bar"` puts the selection's words in the bar; a program's
    /// go there whatever it says.** Perturbation: read the setting in
    /// `raise_program`.
    #[test]
    fn a_program_has_no_selection_to_be_placed_by() {
        let t = zero();
        let mut bar = Message::default();
        bar.copy.place = MessagePlace::Bar;
        let mut s = signal(bar);
        s.selection(Characters(5), Pulse::Skip);
        let f = drawn(&mut s, t).look.unwrap();
        assert_eq!(
            (f.lines[0].row, f.lines[0].col),
            (Row::from_index(23), Col::ZERO)
        );
        let mut s = signal(Message::default());
        assert_eq!(s.program(batch("hello"), t), Owes::Frame);
        let f = drawn(&mut s, t).look.unwrap();
        assert_eq!(
            (f.lines[0].row, f.lines[0].col),
            (Row::from_index(23), Col::ZERO)
        );
        assert_eq!(said(&f).trim(), "A program copied 5 characters");
    }

    /// **`enable = "off"` stops the words, the pulse and the announcement,
    /// and a reload takes down what is up.** Perturbation: drop either check
    /// on `CopySignal::Off`.
    #[test]
    fn off_says_nothing_and_a_reload_takes_it_down() {
        let t = zero();
        let mut off = Message::default();
        off.copy.enable = CopySignal::Off;
        let mut s = signal(off);
        assert_eq!(s.selection(Characters(5), Pulse::Run), Owes::Nothing);
        assert_eq!(s.program(batch("x"), t), Owes::Nothing);
        assert_eq!(drawn(&mut s, t).look, None);
        assert_eq!(s.take_saying(), None);
        let mut s = signal(Message::default());
        s.selection(Characters(5), Pulse::Run);
        assert!(drawn(&mut s, t).look.is_some());
        s.set(off);
        let f = drawn(&mut s, t + MS);
        assert_eq!((f.look, f.pulse), (None, SelectionPulse::Off));
        assert_eq!(f.owes, Owes::Frame);
        // And on again, with a new place, for the next copy.
        let mut on = Message::default();
        on.copy.place = MessagePlace::Bar;
        s.set(on);
        assert_eq!(s.message(), on);
        s.selection(Characters(5), Pulse::Skip);
        let f = drawn(&mut s, t + 2 * MS).look.unwrap();
        assert_eq!(f.lines[0].row, Row::from_index(23));
    }

    /// **Any key takes the words down, and owes one frame.** The key's way on
    /// to the program is the next test's. Perturbation: return `Nothing` from
    /// `dismiss`.
    #[test]
    fn a_key_dismisses_the_words() {
        let t = zero();
        let mut s = signal(Message::default());
        assert_eq!(s.dismiss(), Owes::Nothing, "nothing up");
        s.selection(Characters(5), Pulse::Skip);
        drawn(&mut s, t);
        assert_eq!(s.dismiss(), Owes::Frame);
        assert_eq!(s.deadline(), None, "no end owed once it is gone");
        assert_eq!(drawn(&mut s, t + MS).look, None);
    }

    /// **A key typed takes the words down and its release does not**, so
    /// letting go of a chord's Ctrl under kitty's report of releases leaves
    /// them up. Perturbation: read every event routed as typed.
    #[test]
    fn a_release_does_not_dismiss_the_words() {
        use crate::pressed::Route;
        use baud_domain::{KeyCode, KeyEvent, KeyEventKind, Modifiers};
        let press = KeyEvent::press(KeyCode::Escape, Modifiers::NONE);
        let release = KeyEvent {
            kind: KeyEventKind::Release,
            ..press
        };
        let repeat = KeyEvent {
            kind: KeyEventKind::Repeat,
            ..press
        };
        assert!(Route::Program(press).typed());
        assert!(Route::Field(press).typed());
        assert!(Route::Program(repeat).typed());
        assert!(!Route::Program(release).typed());
        assert!(!Route::Field(release).typed());
        assert!(!Route::Nobody.typed());
    }

    /// **"Copied" is said only for a copy the pasteboard kept**: the arm
    /// for a write the pasteboard did not keep says none of a copy's words,
    /// and says the not-copied words through the function that raises them.
    /// Perturbation: fold that arm into the taken one.
    #[test]
    fn the_copied_words_come_only_from_a_confirmed_copy() {
        let head = concat!("written: clipboard::Written::", "Unconfirmed(");
        let src = crate::app_source::within(crate::app_source::app_source(), head);
        let at = src.find(head).expect("the arm for a write not kept");
        let arm = &src[at..];
        let arm = arm
            .find("\n                Outcome::")
            .map_or(arm, |end| &arm[..end]);
        assert!(!arm.contains("selection_copied("), "{arm}");
        assert!(!arm.contains("programs.copied("), "{arm}");
        assert!(arm.contains("self.say_not_copied()"), "{arm}");
        let said = body(
            crate::app_source::app_source(),
            concat!("    fn say_not_copied", "(&mut self) {"),
        );
        assert!(said.contains("app.copied.not_copied()"), "{said}");
    }

    /// **A copy not kept says so, in the bar and to a screen reader, and
    /// none of a copy's words.** Perturbation: say a count.
    #[test]
    fn a_copy_not_kept_says_not_copied() {
        let mut s = signal(Message::default());
        assert_eq!(s.not_copied(), Owes::Frame);
        let said = s.take_saying().unwrap_or_default();
        assert_eq!(said, "Not copied: the clipboard kept what it had");
        assert!(!said.starts_with("Copied"), "{said}");
    }

    /// The text of the function `head` starts, from `src`.
    fn body<'s>(src: &'s str, head: &str) -> &'s str {
        let src = crate::app_source::within(src, head);
        let at = src.find(head).unwrap_or_else(|| panic!("{head}"));
        let rest = &src[at..];
        rest.split_once("\n    }\n").map_or(rest, |(b, _)| b)
    }

    /// **The dismissing key still reaches the program**: each driver takes
    /// the words down on no branch that returns, ahead of what sends the key
    /// on. macOS's `key` hears key-downs alone. Perturbations: add a `return`
    /// after the dismissal; move it past `Term::key` on Linux.
    #[test]
    fn the_dismissing_key_reaches_the_program() {
        let mac = body(
            crate::app_source::app_source(),
            concat!("    fn key(&mut self, ", "key: &KeyPress) -> Handled {"),
        );
        let gone = mac.find("self.app.copied.dismiss()").expect("the mac key");
        assert!(
            !mac[..gone].contains("return"),
            "nothing returns ahead of it"
        );
        let rest = &mac[gone..];
        let next = rest.find("if key.modifiers.command {").expect("the key");
        assert!(!rest[..next].contains("return"), "nor just after it");
        assert!(
            rest.contains("self.typed(e, Some(VirtualKey(key.key_code)));"),
            "it goes on to the core"
        );
        let linux = body(
            include_str!("linux.rs"),
            concat!(
                "    fn route_key(&mut self, ",
                "route: crate::pressed::Route) {"
            ),
        );
        let gone = linux.find("self.copied.dismiss()").expect("the linux key");
        let rest = &linux[gone..];
        let next = rest.find("let k = match route {").expect("the route");
        assert!(
            !rest[..next].contains("return"),
            "nothing returns ahead of it"
        );
        assert!(
            rest.contains("crate::reads::input(term, |t| t.key(k), |b| outbox.push(b))"),
            "it goes on to the core"
        );
    }

    /// **Under the link preview no words are drawn, none wait, and the
    /// address keeps the bar**: words up when it opens go, a program's copy
    /// under it never shows, and when it closes nothing comes back. Its
    /// announcement is not the bar's and still comes, paced. Perturbations:
    /// keep `up` under `Preview::Up`; queue the words for after it.
    #[test]
    fn a_program_never_replaces_the_address_under_a_preview() {
        let t = zero();
        let mut s = signal(Message::default());
        s.selection(Characters(5), Pulse::Skip);
        assert!(drawn(&mut s, t).look.is_some());
        let f = s.frame(t + MS, Preview::Up, Some(&selected()), cols(), rows());
        assert_eq!(f.look, None, "words up when it opens go");
        assert_eq!(f.owes, Owes::Frame);
        s.take_saying();
        s.program(batch("secret"), t + 2 * MS);
        let f = s.frame(t + 2 * MS, Preview::Up, Some(&selected()), cols(), rows());
        assert_eq!(f.look, None, "a program's copy never shows under it");
        assert!(s.take_saying().is_some(), "it is still announced");
        let f = drawn(&mut s, t + 3 * MS);
        assert_eq!(f.look, None, "and nothing was queued for after it");
        // The drivers hand the renderer the preview's bar and this frame
        // nothing, from one reading.
        for (src, reading) in [
            (crate::app_source::app_source(), "self.hover.look()"),
            (include_str!("linux.rs"), "self.plan_text.hover.as_ref()"),
        ] {
            let want = format!("crate::copied::Preview::of({reading})");
            assert_eq!(src.matches(&want).count(), 1, "{want}");
        }
    }

    /// **A program looping `OSC 52` costs one frame and one announcement each
    /// 1.5 s, and the bar ends on its last count.** Sixty batches a second for
    /// ten seconds, each with a frame asked for, the count differing every
    /// time: the frames the signal itself owes and the words spoken are
    /// counted. Perturbations: raise on every batch rather than hold, which
    /// owes 600 frames; drop the gate in `speak_program`, or restart `until`
    /// on a held copy, which never ends.
    #[test]
    fn a_program_looping_copies_cannot_flood_frames_or_announcements() {
        let t0 = zero();
        let mut s = signal(Message::default());
        let (mut frames, mut spoken) = (0, 0);
        let mut last = None;
        for i in 0..600u32 {
            let now = t0 + i * Duration::from_micros(16_667);
            let text = "x".repeat(usize::try_from(i).unwrap() + 1);
            s.program(batch(&text), now);
            let f = drawn(&mut s, now);
            frames += usize::from(f.owes == Owes::Frame);
            spoken += usize::from(s.take_saying().is_some());
            last = f.look.or(last);
        }
        // Ten seconds at one change each 1.5 s, rounded up, the first frame
        // included.
        assert!((6..=8).contains(&frames), "{frames} frames");
        assert!((6..=8).contains(&spoken), "{spoken} announcements");
        let shown = said(&last.unwrap());
        assert!(shown.starts_with(" A program copied"), "{shown}");
        // Quiet after the loop: the held copy shows once and it ends.
        let quiet = t0 + Duration::from_secs(20);
        assert!(drawn(&mut s, quiet).look.is_some(), "the last held");
        assert_eq!(drawn(&mut s, quiet + MESSAGE_FOR).look, None);
        assert_eq!(s.deadline(), None);
    }

    /// **A person's copy is not held behind a program's**, and the program's
    /// held count shows after it. Perturbation: hold a selection like a
    /// program's copy.
    #[test]
    fn a_person_is_not_held_behind_a_program() {
        let t = zero();
        let mut s = signal(Message::default());
        s.program(batch("abc"), t);
        drawn(&mut s, t);
        s.take_saying();
        assert_eq!(s.selection(Characters(9), Pulse::Skip), Owes::Frame);
        assert_eq!(s.take_saying().as_deref(), Some("Copied 9 characters"));
        s.program(batch("ab"), t + MS);
        let f = drawn(&mut s, t + 2 * MS).look.unwrap();
        assert_eq!(said(&f).trim(), "Copied 9 characters");
        let f = drawn(&mut s, t + 2 * MS + MESSAGE_FOR).look.unwrap();
        assert_eq!(said(&f).trim(), "A program copied 2 characters");
    }

    /// **A frame whose present failed is owed again, its start and its end,
    /// at a pace; a presented one is not.** Perturbations: clear `unpresented`
    /// in `frame`, and the end is lost; retry with no delay, which forces a
    /// frame each tick.
    #[test]
    fn a_failed_present_never_loses_the_end() {
        let t = zero();
        let mut s = signal(Message::default());
        s.selection(Characters(5), Pulse::Skip);
        drawn(&mut s, t);
        s.presented();
        let end = t + MESSAGE_FOR;
        assert_eq!(drawn(&mut s, end).look, None);
        s.refused(end);
        assert_eq!(s.deadline(), Some(end + RETRY), "owed again, at a pace");
        assert!(!s.is_due(end + RETRY - MS));
        assert!(s.is_due(end + RETRY));
        // Failing again keeps it owed.
        drawn(&mut s, end + RETRY);
        s.refused(end + RETRY);
        assert_eq!(s.deadline(), Some(end + RETRY + RETRY));
        // Presented: nothing more.
        drawn(&mut s, end + RETRY + RETRY);
        s.presented();
        s.refused(end + RETRY + RETRY);
        assert_eq!(s.deadline(), None);
        // A frame that changed nothing owes nothing however it fared.
        let mut idle: Signal<Instant> = signal(Message::default());
        drawn(&mut idle, t);
        idle.presented();
        drawn(&mut idle, t + MS);
        idle.refused(t + MS);
        assert_eq!(idle.deadline(), None);
    }

    /// **The pulse: 150 ms from its first frame, a frame to start and one to
    /// end, only for the selection, and with no selection on the view none.**
    /// Perturbations: run it for a program's copy; hold it past `PULSE_FOR`.
    #[test]
    fn a_pulse_runs_its_length_and_only_for_the_selection() {
        let t = zero();
        let mut s = signal(Message::default());
        s.selection(Characters(5), Pulse::Run);
        let f = drawn(&mut s, t);
        assert_eq!(f.pulse, SelectionPulse::On);
        assert_eq!(s.deadline(), Some(t + PULSE_FOR));
        let f = drawn(&mut s, t + PULSE_FOR);
        assert_eq!(f.pulse, SelectionPulse::Off);
        assert_eq!(f.owes, Owes::Frame);
        assert!(f.look.is_some(), "the words stay");
        let mut s = signal(Message::default());
        s.program(batch("abc"), t);
        assert_eq!(drawn(&mut s, t).pulse, SelectionPulse::Off);
        let mut s = signal(Message::default());
        s.selection(Characters(5), Pulse::Run);
        let f = s.frame(t, Preview::None, None, cols(), rows());
        assert_eq!(f.pulse, SelectionPulse::Off, "nothing on the view to flash");
    }

    /// **The pulse's settings**: on asks, off does not; Reduce Motion, Dim
    /// Flashing Lights and both colours `"none"` each stop it, and the words
    /// take no part. Perturbation: drop any clause of `pulse_runs`.
    #[test]
    fn reduce_motion_and_dim_flashing_lights_stop_the_pulse_alone() {
        use FlashingLights::{Dimmed, Full};
        use SystemMotion as M;
        let mut on = Message::default();
        on.copy.pulse = CopyPulse::On;
        assert!(pulse_runs(on, M::Full, Full));
        assert!(
            !pulse_runs(Message::default(), M::Full, Full),
            "off by default"
        );
        assert!(!pulse_runs(on, M::Reduced, Full), "Reduce Motion");
        assert!(!pulse_runs(on, M::Full, Dimmed), "Dim Flashing Lights");
        let none = Message {
            colours: MessageColours {
                page: MessagePaint::Selection,
                text: MessagePaint::Selection,
            },
            ..on
        };
        assert!(!pulse_runs(none, M::Full, Full), "nothing of its own");
        let mut off = on;
        off.copy.enable = CopySignal::Off;
        assert!(!pulse_runs(off, M::Full, Full), "off stops the pulse");
        // The words under Reduce Motion: a skipped pulse leaves them.
        let mut s = signal(on);
        s.selection(Characters(5), Pulse::Skip);
        let f = drawn(&mut s, zero());
        assert_eq!(f.pulse, SelectionPulse::Off);
        assert!(f.look.is_some(), "the words show as usual");
    }

    /// **A batch's last copy is the one it says**: on macOS the copies
    /// whose outcomes came back in one drain of the effect queue
    /// (`turns_answered`). Perturbation: keep the first in `copied`.
    #[test]
    fn a_batch_counts_its_last_copy() {
        let mut b = Batch::default();
        assert_eq!(b, Batch::default(), "none yet");
        b.copied("abc");
        assert_eq!(b, batch("xyz"));
        b.copied("ab");
        assert_eq!(b, batch("xy"), "the last one counts");
    }

    /// **Each copy is spoken in the same words, once; off speaks nothing.**
    /// Perturbation: speak `said` from the frame rather than the copy.
    #[test]
    fn the_announcement_is_the_same_words_once() {
        let t = zero();
        let mut s = signal(Message::default());
        s.selection(Characters(1), Pulse::Skip);
        assert_eq!(s.take_saying().as_deref(), Some("Copied 1 character"));
        assert_eq!(s.take_saying(), None, "once");
        s.dismiss();
        s.program(batch("hi"), t);
        assert_eq!(
            s.take_saying().as_deref(),
            Some("A program copied 2 characters")
        );
    }

    /// **Both drivers drive the signal**: every copy path says it and neither
    /// says it on a refused write, the deadline owes a frame, and the frame
    /// hands the renderer its look and pulse. Perturbation: move any line out
    /// of its driver.
    #[test]
    fn both_drivers_drive_the_signal() {
        const MAC: &str = crate::app_source::app_source();
        const LINUX: &str = include_str!("linux.rs");
        let copy = body(MAC, concat!("    fn copy(", "&mut self) {"));
        // Cmd-C asks the effect queue; its outcome, taken, is said.
        assert!(copy.contains("CopyFrom::Selection"), "{copy}");
        assert_eq!(
            MAC.matches("CopyFrom::Selection => self.selection_copied(text.as_str()),")
                .count(),
            1,
            "the selection said once the pasteboard took it"
        );
        for (src, wants) in [
            (
                MAC,
                [
                    "self.app.copied.is_due(now)",
                    ".frame(seen, preview, highlight.as_ref(), cols, rows);",
                    "self.renderer.set_selection_pulse(signal.pulse);",
                    "self.app.copied.program(programs, host_now())",
                ],
            ),
            (
                LINUX,
                [
                    "d.copied.is_due(at)",
                    ".frame(seen_at, preview, highlight.as_ref(), cols, rows);",
                    "self.renderer.set_selection_pulse(signal.pulse);",
                    "self.copied.program(copies, Instant::now())",
                ],
            ),
        ] {
            for want in wants {
                assert_eq!(src.matches(want).count(), 1, "{want}");
            }
        }
        let linux = body(
            LINUX,
            concat!("    fn copy_selection(", "&mut self, board: Board) {"),
        );
        assert!(
            linux.contains("self.copy_asked(&text, ") && !linux.contains("selection_copied"),
            "the Linux copy is asked, and says nothing yet"
        );
        let answered = body(
            LINUX,
            concat!(
                "    fn copy_answered(",
                "&mut self, b: Board, took: crate::copied::Took) {"
            ),
        );
        assert!(
            answered.contains("self.selection_copied(count, Some(to));")
                && answered.contains("self.copied.not_copied()"),
            "the Linux copy says it once the selections answered"
        );
        // To VoiceOver through the window's effect queue, whose turn says
        // them.
        assert_eq!(
            MAC.matches("Spoken::of(words)").count(),
            1,
            "the words asked for"
        );
        assert_eq!(
            include_str!("os_effect.rs")
                .matches("baud_present_macos::announce::announce(t.main_thread(), words.as_str());")
                .count(),
            1,
            "the words reach VoiceOver"
        );
        let mac_reload = MAC.contains("Part::Message => {");
        let linux_reload = LINUX.contains("Part::Message => {");
        assert!(mac_reload && linux_reload, "a reload reaches both");
    }
}
