//! **What the shell says it runs**, folded from the core's record of each
//! part of a read: the semantic prompt marks' (OSC 133) `OSC 133 ; C` with
//! its `cmdline=` or `cmdline_url=` key (`[[source: kitty-shell-integration]]`),
//! and the `OSC 133 ; D` that ends the command. The core reads each mark
//! as it dispatches it -- what it says of the window's shell token and what
//! a `C`'s command line names (`baud_core::MarkEvent`) -- and [`Fold`]
//! steps what is open over them in order (`baud_domain::MarkOpen::step`).
//! No byte of a read is walked here for marks.
//!
//! A command whose program is one not on this machine -- ssh and its kin,
//! `baud_domain::ELSEWHERE` -- makes every part from its `C` to its `D`
//! remote, for a link's origin and a printed path's mark, on those parts'
//! rows. The mark comes in the stream before the command's first byte, so
//! a job that starts, prints a link and ends between two samples of the
//! group in front is still named, on both systems and with no call to the
//! kernel. Each part is taken as one [`Ran`]: the remote command's, a local
//! command's, or the shell's own -- what follows the shell's `D`, with its
//! token, its prompt after a command, which the program sampled before it
//! did not write; any other part under no command is the samples' to
//! decide.
//!
//! **A mark can only raise.** A remote program can print marks too, so
//! nothing in a mark lowers what the parts are taken as unless it carries
//! the token baud made for the window's shell ([`ShellToken`]), handed to
//! its integration at the spawn and to the core (`Term::set_shell_token`),
//! never to anything after: `baud_token=` on its `C` and its `D`. The core
//! never takes one from the stream, so a mark with any other is a
//! program's. A program on the other end cannot end the command early with
//! a `D` of its own, and a `C` of the shell's naming a program here lowers
//! only a writer gone before it was named (`paths::Paths::written_here`),
//! never one named not local. With no token -- the integration off, a
//! shell marking its own prompts, as fish 4 does, or another integration
//! -- no part is the shell's or a command's named here, so nothing lowers;
//! a plain `D` ends a remote command, and the samples and the process watch
//! are what is left against a program that prints one.
//!
//! **A `C` with no command line, or an empty one, names nothing**
//! (`CommandRuns::Nothing`): it neither opens a remote command nor ends
//! one, and the samples and the process watch decide. bash sends one when
//! its history kept no line for the command, as `history 1` is then
//! another command's; zsh when its history mechanism is not active.
//!
//! **What the record does not hold is not guessed.** A part with more marks
//! than the record keeps is overflowed ([`Timeline::overflowed`]): its
//! marks do not match the core's (`vouch::Matched`), and the window's marks
//! are not trusted until the shell's `D` with its token re-syncs them
//! (`vouch::Latch`).

use baud_core::{BytePos, Fill, ReadRecord};
use baud_domain::{MarkBack, MarkOpen, StreamByte, TokenSeen, WalkSteps};

/// **The token baud makes for one window's shell**: sixteen hex digits of
/// the system's random source, handed to the integration at the spawn
/// (`shell::inject`), which takes it out of the environment before the
/// user's startup files run and sends it on its own `C` and `D` marks, and
/// to the core as its hash ([`ShellToken::hash`]). No program on the other
/// end of an ssh can know it. Made only by [`ShellToken::mint`], as the
/// sixteen digits a [`baud_domain::ShellTokenHex`] holds, so its hash is
/// always there; `Debug` shows none of them.
#[derive(Clone, PartialEq, Eq)]
pub struct ShellToken(baud_domain::ShellTokenHex);

/// The digits are not shown: the token sits in the reader thread and in
/// the spawn's records, and a log of either holds no secret.
impl core::fmt::Debug for ShellToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ShellToken(..)")
    }
}

impl ShellToken {
    /// A new token, none when the random source cannot be read: the window's
    /// shell is then marked with none, and nothing it marks lowers.
    pub fn mint() -> Option<ShellToken> {
        use std::io::Read;
        let mut raw = [0u8; 8];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut raw))
            .ok()?;
        let text: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        baud_domain::ShellTokenHex::parse(text.as_bytes()).map(ShellToken)
    }

    /// Its text, as the integration sends it.
    pub fn text(&self) -> String {
        self.0.digits().iter().map(|&b| char::from(b)).collect()
    }

    /// Its digits, as the link sniff finds the shell's `D` by them.
    pub fn hex(&self) -> baud_domain::ShellTokenHex {
        self.0
    }

    /// **What the core reads each mark's token against**
    /// (`Term::set_shell_token`): the hash of its sixteen digits.
    pub fn hash(&self) -> baud_domain::ShellTokenHash {
        baud_domain::ShellTokenHash::of(&self.0)
    }

    /// A token spelled `s`, sixteen hex digits, for a test.
    #[cfg(test)]
    pub fn given(s: &str) -> ShellToken {
        ShellToken(baud_domain::ShellTokenHex::parse(s.as_bytes()).expect("sixteen hex digits"))
    }
}

/// **What the shell's marks say of a part** ([`Fold::part`]), which the
/// paths file into the part's own rows (`probe::Probes::read_taken`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Ran {
    /// No token for the window's shell, or none since `exec` took it: the
    /// samples and the process watch decide.
    #[default]
    Unmarked,
    /// The shell's own: a part carrying the shell's `D`, with its token,
    /// and nothing open after it, and of its bytes only those after that
    /// mark -- the prompt after a command. What the group sampled before
    /// the read wrote is not in it. A part with no such mark is not the
    /// shell's, whatever is open: output under no command, a background
    /// job's at the prompt, or a shell marking its prompts without the
    /// token, as fish 4 does, is [`Ran::Unmarked`]; and so is the shell's
    /// own redraw of its prompt, or a key it echoes, which the samples
    /// then decide.
    Shell,
    /// Written while a command the shell, with its token, named a program
    /// on this machine ran: a writer gone before it was named is local.
    Here,
    /// Written, at any point, while a command the shell named not local
    /// ran -- the part of its `C` and of its `D` included.
    Elsewhere,
    /// Written while a command whose `C` named nothing ran: the samples and
    /// the process watch decide.
    Command,
}

/// How much a [`Ran`] lets a part's rows be lowered, least first: what
/// [`Ran::or`] keeps the most of.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Caution {
    /// The shell's own: the group sampled before is not its writer.
    Shell,
    /// A command named here: a writer gone before it was named is lifted.
    Here,
    /// Nothing named: the samples and the watch decide.
    Samples,
    /// A command named not local: the rows are remote.
    Remote,
}

impl Ran {
    fn caution(self) -> Caution {
        match self {
            Ran::Shell => Caution::Shell,
            Ran::Here => Caution::Here,
            Ran::Unmarked | Ran::Command => Caution::Samples,
            Ran::Elsewhere => Caution::Remote,
        }
    }

    /// **The more cautious of two parts' verdicts**, for parts filed over
    /// one span, as a drain of them is on macOS (`probe::Probes::drain_taken`):
    /// a command named not local over one named nothing, over one named
    /// here, over the shell's own, so no part's verdict lowers the rows
    /// another part in the span wrote. The shell's only if every part is.
    pub fn or(self, other: Ran) -> Ran {
        match other.caution() > self.caution() {
            true => other,
            false => self,
        }
    }
}

/// **Whether a part fed a folder report the core acted on**, `OSC 7` or an
/// iTerm2 `CurrentDir` (`baud_core::ReadRecord::folders`), and what was
/// running as the last of them came ([`Timeline::open_at`]): one sent
/// while a command the shell named not local ran is the remote program's,
/// which can name this machine as well as the shell can, and is not taken
/// (`probe::Probes::read_taken`). The shell's own after that command's
/// `D`, in the same part as the `D` and the prompt, is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Folder {
    #[default]
    Unreported,
    Reported,
    FromRemote,
}

/// **What was open after a mark**, as [`Fold`] stepped it: nothing, a
/// command the shell named here or naming nothing, or one named not local.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Left {
    #[default]
    Closed,
    Here,
    Remote,
}

impl Left {
    fn of(open: MarkOpen) -> Left {
        match open {
            MarkOpen::No => Left::Closed,
            MarkOpen::Here(_) | MarkOpen::Unnamed(_) => Left::Here,
            MarkOpen::Remote(_) => Left::Remote,
        }
    }
}

/// **One `OSC 133` mark the core recorded**: where its terminator is in the
/// read (`baud_core::MarkEvent::at`), its letter, the first byte after
/// `133;`, whether it was the shell's `D` with its token, and what was
/// open after it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    at: BytePos,
    letter: StreamByte,
    shells: ShellsD,
    left: Left,
}

impl Mark {
    pub fn at(&self) -> BytePos {
        self.at
    }
    pub fn letter(&self) -> StreamByte {
        self.letter
    }
    /// Whether it was the shell's `D`, carrying the window's token.
    pub fn shells_d(&self) -> ShellsD {
        self.shells
    }
    /// What was open after it.
    pub fn left(&self) -> Left {
        self.left
    }
}

/// The offset as a number and the letter as a character.
impl core::fmt::Debug for Mark {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}{}@{}{:?}",
            char::from(self.letter.raw_byte()),
            match self.shells {
                ShellsD::Yes => "*",
                ShellsD::No => "",
            },
            self.at.index(),
            self.left
        )
    }
}

/// How many marks a [`Timeline`] keeps: as many as the core records of a
/// part (`baud_core::ReadMarks`).
const MARKS: usize = 8;

/// **The marks a part carried, where each ended, and what was open as it
/// began**, as the core recorded them, each checked against that record
/// offset for offset and letter for letter (`vouch::Matched`). Eight are
/// kept; past them the record overflowed, and the part is unmatched.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Timeline {
    began: Left,
    kept: [Option<Mark>; MARKS],
    /// Where the first mark not kept ended: every part reaching past it
    /// holds marks no timeline names.
    over: Option<BytePos>,
}

/// Each mark, and where the first dropped one ended.
impl core::fmt::Debug for Timeline {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let kept: Vec<Mark> = self.marks().collect();
        f.debug_struct("Timeline")
            .field("began", &self.began)
            .field("kept", &kept)
            .field("over", &self.over.map(BytePos::index))
            .finish()
    }
}

impl Timeline {
    fn begun(began: Left) -> Timeline {
        Timeline {
            began,
            ..Timeline::default()
        }
    }

    fn push(&mut self, m: Mark) {
        match self.kept.iter_mut().find(|s| s.is_none()) {
            Some(slot) => *slot = Some(m),
            None => {
                self.over.get_or_insert(m.at);
            }
        }
    }

    /// The marks, in the order the part ended them.
    pub fn marks(&self) -> impl Iterator<Item = Mark> + '_ {
        self.kept.iter().map_while(|m| *m)
    }

    /// Whether the part held marks past the kept ones.
    pub fn overflowed(&self) -> bool {
        self.over.is_some()
    }

    /// **What was open at `at`**: after the last mark that ended before
    /// it, or as the part began.
    pub fn open_at(&self, at: BytePos) -> Left {
        self.marks()
            .filter(|m| m.at < at)
            .last()
            .map_or(self.began, |m| m.left)
    }
}

/// Whether a part carried the shell's `D`, with its token.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ShellsD {
    #[default]
    No,
    Yes,
}

/// **What is open across the parts of the window's reads**, stepped at each
/// mark the core recorded, in order: one per window, beside the terminal
/// that records them (`reads::Pace`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Fold {
    open: MarkOpen,
    /// The marks folded so far, each one step.
    walked: WalkSteps,
}

/// **What the marks of one part say** ([`Fold::part`]): the part's
/// verdict, its marks, whether it carried the shell's `D`, and, where the
/// first such `D` began in the part, where and what the bytes from it on
/// say, read as a part of their own (`cut.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Folded {
    pub ran: Ran,
    pub marks: Timeline,
    pub shells_d: ShellsD,
    after: Option<(BytePos, Ran)>,
}

/// The `D`'s offset shown as a number.
impl core::fmt::Debug for Folded {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Folded")
            .field("ran", &self.ran)
            .field("marks", &self.marks)
            .field("shells_d", &self.shells_d)
            .field("after", &self.after.map(|(d, r)| (d.index(), r)))
            .finish()
    }
}

impl Folded {
    /// **What the bytes from the shell's `D` on say, where the core placed
    /// that `D` at `d`**: none where the part's first `D` of the shell's
    /// began elsewhere, or before the part, or where it carried none.
    pub fn after_at(&self, d: BytePos) -> Option<Ran> {
        self.after
            .filter(|&(at, _)| at == d)
            .map(|(_, after)| after)
    }

    /// Where the part's first `D` of the shell's began, when in the part,
    /// and what the bytes from it on say.
    #[cfg(test)]
    pub fn after(&self) -> Option<(BytePos, Ran)> {
        self.after
    }
}

/// Where a part's first `D` of the shell's, with its token, began.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DAt {
    Unseen,
    At(BytePos),
    /// Before the part: the `D` was split between two.
    Earlier,
}

/// **What a part was written under, from its start and from its shell's
/// `D` on**: the second only once that `D` came.
#[derive(Clone, Copy, Default, Debug)]
struct Outs {
    all: Out,
    after: Option<Out>,
}

impl Outs {
    fn saw(&mut self, open: MarkOpen, what: Wrote, shells_d: ShellsD) {
        self.all.saw(open, what, shells_d);
        if let Some(after) = self.after.as_mut() {
            after.saw(open, what, shells_d);
        }
    }
}

impl Fold {
    /// **What the marks the core recorded of the part from `from` to `to`
    /// say** ([`Ran`]): written at any point while a command the shell
    /// named not local ran -- open as the part began, or opened by a mark
    /// inside it, as a short ssh's `C`, its link and its `D` can all be one
    /// part; else what wrote the bytes that are not the shell's marks -- a
    /// command the shell named here, one that named nothing, or none: the
    /// shell's own only after its `D`, with its token, and with no command
    /// open after it ([`Ran::Shell`]); any other part under none is
    /// unmarked, the samples'. **A byte no recorded mark covers is
    /// output**: text, another sequence, another control string, a mark
    /// the core did not act on. Each mark is one step and no byte is read.
    pub fn part(&mut self, rec: &ReadRecord, from: BytePos, to: BytePos) -> Folded {
        let mut marks = Timeline::begun(Left::of(self.open));
        let mut shells_d = ShellsD::No;
        let mut d = DAt::Unseen;
        let mut out = Outs::default();
        out.saw(self.open, Wrote::Marks, shells_d);
        // The first byte no mark covers.
        let mut next = from;
        for m in rec.marks().iter() {
            self.walked.bump();
            let began = match m.back() {
                MarkBack::Within(back) => m.at().index().checked_sub(back.index()),
                MarkBack::Before => None,
            }
            .map(BytePos::from_index)
            .filter(|&b| b >= from);
            if began.unwrap_or(from) > next {
                out.saw(self.open, Wrote::Output, shells_d);
            }
            self.open = self.open.step(m.letter(), m.token(), m.runs());
            let shells = match (m.letter().raw_byte(), m.token()) {
                (b'D', TokenSeen::Shells) => ShellsD::Yes,
                _ => ShellsD::No,
            };
            if shells == ShellsD::Yes {
                shells_d = ShellsD::Yes;
                if d == DAt::Unseen {
                    d = began.map_or(DAt::Earlier, DAt::At);
                }
            }
            marks.push(Mark {
                at: m.at(),
                letter: m.letter(),
                shells,
                left: Left::of(self.open),
            });
            // From the shell's first `D` on, what the bytes say is read
            // again, as a part of their own would be.
            if out.after.is_none() && d != DAt::Unseen {
                let mut after = Out::default();
                after.saw(self.open, Wrote::Marks, shells_d);
                out.after = Some(after);
            }
            out.saw(self.open, Wrote::Marks, shells_d);
            next = next.max(BytePos::from_index(m.at().index() + 1));
        }
        if rec.marks().fill() == Fill::Overflowed {
            marks.over.get_or_insert(next);
        }
        if to > next {
            out.saw(self.open, Wrote::Output, shells_d);
        }
        let after = match d {
            DAt::At(at) => Some((at, out.after.unwrap_or_default().ran(self.open, shells_d))),
            DAt::Unseen | DAt::Earlier => None,
        };
        Folded {
            ran: out.all.ran(self.open, shells_d),
            marks,
            shells_d,
            after,
        }
    }

    /// **The marks folded so far**, one step each: what the window's marks
    /// cost, counted, none for a byte.
    #[cfg(test)]
    pub fn walked(&self) -> WalkSteps {
        self.walked
    }
}

/// Whether bytes in a part were the shell's marks or a program's output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wrote {
    Marks,
    Output,
}

/// **What a part was written under, as its marks went**: the weightiest
/// command any of it was written under, in this order -- none, or none
/// after the shell's own `D`; one the shell named here; none with no such
/// `D` before it, the samples' to decide; one naming nothing; one named not
/// local.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Out {
    #[default]
    Nothing,
    Here,
    Unmarked,
    Unnamed,
    Remote,
}

impl Out {
    /// `open` was open while the part wrote `what`, `shells_d` whether the
    /// part had carried the shell's own `D` by then: a command named not
    /// local counts whatever was written under it, the others only output,
    /// and output under none is the shell's only after that `D`.
    fn saw(&mut self, open: MarkOpen, what: Wrote, shells_d: ShellsD) {
        let under = match (open, what, shells_d) {
            (MarkOpen::Remote(_), _, _) => Out::Remote,
            (MarkOpen::Here(_), Wrote::Output, _) => Out::Here,
            (MarkOpen::Unnamed(_), Wrote::Output, _) => Out::Unnamed,
            (MarkOpen::No, Wrote::Output, ShellsD::No) => Out::Unmarked,
            (MarkOpen::No, Wrote::Output, ShellsD::Yes)
            | (MarkOpen::No | MarkOpen::Here(_) | MarkOpen::Unnamed(_), Wrote::Marks, _) => {
                Out::Nothing
            }
        };
        *self = (*self).max(under);
    }

    /// The part's verdict, `open` what is open as it ends, `shells_d`
    /// whether it carried the shell's own `D`: a part that wrote nothing
    /// but marks, or nothing but after that `D`, is the shell's only if it
    /// carried the `D` and no command is open after it, and the open
    /// command's while one is, so a `D` without the token, a body left open
    /// at the part's end, or a part with no `D` at all, is not the shell's.
    /// A command here, or one naming nothing, is open only by the shell's
    /// token, and a remote one makes the part remote, so with no token no
    /// part is the shell's or a command's named here.
    fn ran(self, open: MarkOpen, shells_d: ShellsD) -> Ran {
        match self {
            Out::Remote => Ran::Elsewhere,
            Out::Unnamed => Ran::Command,
            Out::Unmarked => Ran::Unmarked,
            Out::Here => Ran::Here,
            Out::Nothing => match open {
                MarkOpen::No if shells_d == ShellsD::Yes => Ran::Shell,
                MarkOpen::No => Ran::Unmarked,
                MarkOpen::Here(_) => Ran::Here,
                MarkOpen::Unnamed(_) => Ran::Command,
                MarkOpen::Remote(_) => Ran::Elsewhere,
            },
        }
    }
}

/// **What a `C` mark's command line reads back as**, for the shells' own
/// tests: the line of the first of its fields that carries one, decoded.
#[cfg(test)]
pub(crate) fn command_line(fields: &[u8]) -> Option<Vec<u8>> {
    fields.split(|&b| b == b';').find_map(field_line)
}

/// **The command line one of a `C` mark's fields carries**, decoded:
/// `cmdline=`, "encoded by %q", one shell word read back, or
/// `cmdline_url=`, "UTF-8 URL %-escaped text". None for any other field.
#[cfg(test)]
fn field_line(f: &[u8]) -> Option<Vec<u8>> {
    let eq = f.iter().position(|&b| b == b'=')?;
    let v = &f[eq + 1..];
    match &f[..eq] {
        b"cmdline_url" => Some(url_decoded(v)),
        b"cmdline" => Some(first_word(v).unwrap_or_default()),
        _ => None,
    }
}

/// The first shell word of `s`, unquoted: what `printf %q` of a whole
/// command line gives back as the line.
#[cfg(test)]
fn first_word(s: &[u8]) -> Option<Vec<u8>> {
    word_at(s).map(|(w, _)| w)
}

/// **The next shell word of `s` and what follows it**, unquoted: blanks
/// before it skipped; `'...'` taken as it is, `"..."` with `\` before `"`,
/// `\`, `$` and a backquote, `$'...'` with its C escapes, and `\` outside
/// quotes taking the next byte. A blank, a newline, or one of `;&|()<>`
/// outside quotes ends it. None when nothing but blanks is left.
#[cfg(test)]
fn word_at(s: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let start = s.iter().position(|&b| !matches!(b, b' ' | b'\t'))?;
    let s = &s[start..];
    if matches!(
        s.first(),
        Some(b'\n' | b';' | b'&' | b'|' | b'(' | b')' | b'<' | b'>')
    ) {
        return None;
    }
    let mut w = Vec::new();
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b' ' | b'\t' | b'\n' | b';' | b'&' | b'|' | b'(' | b')' | b'<' | b'>' => break,
            b'\\' => {
                if let Some(&c) = s.get(i + 1) {
                    if c != b'\n' {
                        w.push(c);
                    }
                }
                i += 2;
            }
            b'\'' => {
                let end = s[i + 1..]
                    .iter()
                    .position(|&b| b == b'\'')
                    .map_or(s.len(), |e| i + 1 + e);
                w.extend_from_slice(&s[i + 1..end]);
                i = end + 1;
            }
            b'"' => {
                i += 1;
                while i < s.len() && s[i] != b'"' {
                    if s[i] == b'\\' && matches!(s.get(i + 1), Some(b'"' | b'\\' | b'$' | b'`')) {
                        i += 1;
                    }
                    w.push(s[i]);
                    i += 1;
                }
                i += 1;
            }
            b'$' if s.get(i + 1) == Some(&b'\'') => {
                let rest = ansi_c(&s[i + 2..], &mut w);
                i = s.len() - rest.len();
            }
            c => {
                w.push(c);
                i += 1;
            }
        }
    }
    Some((w, s.get(i..).unwrap_or_default()))
}

/// A `$'...'` string, `s` starting past its opening quote, onto `w`, as
/// bash reads it: `\n`, `\t`, `\r`, `\e`, `\a`, `\\`, `\'`, `\"`, `\xHH` and
/// up to three octal digits. What follows its closing quote.
#[cfg(test)]
fn ansi_c<'a>(s: &'a [u8], w: &mut Vec<u8>) -> &'a [u8] {
    let mut i = 0;
    let digit = |b: u8, radix: u32| {
        char::from(b)
            .to_digit(radix)
            .and_then(|d| u8::try_from(d).ok())
    };
    while i < s.len() && s[i] != b'\'' {
        if s[i] != b'\\' || i + 1 >= s.len() {
            w.push(s[i]);
            i += 1;
            continue;
        }
        let c = s[i + 1];
        i += 2;
        let simple = match c {
            b'n' => Some(b'\n'),
            b't' => Some(b'\t'),
            b'r' => Some(b'\r'),
            b'e' | b'E' => Some(0x1b),
            b'a' => Some(0x07),
            b'\\' | b'\'' | b'"' => Some(c),
            _ => None,
        };
        if let Some(b) = simple {
            w.push(b);
        } else if c == b'x' {
            let mut v = 0u8;
            let mut n = 0;
            while n < 2 {
                let Some(d) = s.get(i).copied().and_then(|b| digit(b, 16)) else {
                    break;
                };
                v = v << 4 | d;
                i += 1;
                n += 1;
            }
            w.push(v);
        } else if let Some(d) = digit(c, 8) {
            let mut v = d;
            let mut n = 1;
            while n < 3 {
                let Some(d) = s.get(i).copied().and_then(|b| digit(b, 8)) else {
                    break;
                };
                v = v.wrapping_shl(3) | d;
                i += 1;
                n += 1;
            }
            w.push(v);
        } else {
            w.extend_from_slice(&[b'\\', c]);
        }
    }
    s.get(i + 1..).unwrap_or_default()
}

/// `v` with each `%XX` decoded, the "cmdline as UTF-8 URL %-escaped text" of
/// `[[source: kitty-shell-integration]]`. A `%` not followed by two hex digits
/// stays as it is.
#[cfg(test)]
fn url_decoded(v: &[u8]) -> Vec<u8> {
    let hex = |b: u8| {
        char::from(b)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
    };
    let mut out = Vec::with_capacity(v.len());
    let mut i = 0;
    while i < v.len() {
        match (
            v[i],
            v.get(i + 1).copied().and_then(hex),
            v.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(h), Some(l)) => {
                out.push(h << 4 | l);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod fed {
    //! **A window's terminal and its fold, fed read by read**, for tests:
    //! each read fed to the core, holding the window's token, its record
    //! taken and folded ([`Fold::part`]), and the link sniff's cut of the
    //! same bytes checked against the core's. Tokens are spelled as sixteen
    //! hex digits ([`hex_of`]), every `baud_token=` value of a read with
    //! them, so a test may write `T`.
    use super::*;
    use baud_core::{Owned, ReadLen, Term};
    use baud_domain::{LinkSniff, ShellTokenHex, WinSize};

    const WS: WinSize = WinSize {
        ws_col: 40,
        ws_row: 6,
    };

    /// A token's value spelled as sixteen hex digits: itself when it is
    /// sixteen, else an FNV-1a hash's, one spelling for each value.
    pub(crate) fn hex_of(v: &[u8]) -> String {
        match v.len() == 16 && v.iter().all(u8::is_ascii_hexdigit) {
            true => String::from_utf8_lossy(v).into_owned(),
            false => {
                let mut h: u64 = 0xcbf2_9ce4_8422_2325;
                for &b in v {
                    h ^= u64::from(b);
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                }
                format!("{h:016x}")
            }
        }
    }

    /// `bytes` with every whole `baud_token=` value spelled ([`hex_of`]),
    /// and where each spelled byte came from.
    fn spelled(bytes: &[u8]) -> (Vec<u8>, Vec<usize>) {
        let mut spelled = Vec::with_capacity(bytes.len());
        let mut from = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let value = i;
            let ends = |b: &u8| matches!(b, b';' | 0x07 | 0x1b | 0x18 | 0x1a);
            let len = match bytes[..i].ends_with(b"baud_token=") {
                // A value the read ends inside is left as it is.
                true => bytes[i..].iter().position(ends).unwrap_or(0),
                false => 0,
            };
            match len {
                0 => {
                    spelled.push(bytes[i]);
                    from.push(i);
                    i += 1;
                }
                _ => {
                    let hex = hex_of(&bytes[i..i + len]);
                    spelled.extend_from_slice(hex.as_bytes());
                    from.extend(std::iter::repeat_n(value, hex.len()));
                    i += len;
                }
            }
        }
        (spelled, from)
    }

    /// Whether each read's sniff cut is checked against the core's.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Cuts {
        Checked,
        /// Bytes the core reads otherwise than the sniff, a picture's body
        /// among them.
        Unchecked,
    }

    /// A window's terminal, holding its shell's token, and its fold.
    pub(crate) struct Shell<'a> {
        pub(crate) term: Term<'a>,
        fold: Fold,
        cuts: Cuts,
        sniff: LinkSniff,
        last: Folded,
        /// The core's record of the last read.
        rec: ReadRecord,
        /// Where each byte of the last read, spelled, came from.
        from: Vec<usize>,
    }

    /// **A window whose shell baud handed `token`**, none for a shell
    /// marked without one, run through `f`, each read's cut checked.
    pub(crate) fn with_shell<R>(token: Option<&str>, f: impl FnOnce(&mut Shell<'_>) -> R) -> R {
        with_shell_cut(token, Cuts::Checked, f)
    }

    /// [`with_shell`], the cuts checked as `cuts` says.
    pub(crate) fn with_shell_cut<R>(
        token: Option<&str>,
        cuts: Cuts,
        f: impl FnOnce(&mut Shell<'_>) -> R,
    ) -> R {
        with_shell_under(baud_domain::ControlStringEnd::default(), token, cuts, f)
    }

    /// [`with_shell_cut`] on a terminal whose control strings end as
    /// `end` says.
    pub(crate) fn with_shell_under<R>(
        end: baud_domain::ControlStringEnd,
        token: Option<&str>,
        cuts: Cuts,
        f: impl FnOnce(&mut Shell<'_>) -> R,
    ) -> R {
        let mut owned = Owned::new(&WS);
        let mut term = Term::new(owned.storage(), &WS);
        term.set_control_string_end(end);
        let hex = token.map(|t| hex_of(t.as_bytes()));
        let parsed = hex
            .as_deref()
            .and_then(|h| ShellTokenHex::parse(h.as_bytes()));
        term.set_shell_token(parsed.map(|t| baud_domain::ShellTokenHash::of(&t)));
        let rec = term.take_record();
        f(&mut Shell {
            term,
            fold: Fold::default(),
            cuts,
            sniff: LinkSniff::new(parsed),
            last: Folded::default(),
            rec,
            from: Vec::new(),
        })
    }

    impl Shell<'_> {
        /// **One read**, fed whole and folded: what the shell's marks say
        /// of it. The sniff's cut of the same bytes is the core's.
        pub(crate) fn read(&mut self, bytes: &[u8]) -> Ran {
            let (spelled, from) = spelled(bytes);
            self.term.write_at(BytePos::ZERO, &spelled);
            self.term
                .feed(BytePos::ZERO, ReadLen::from_index(spelled.len()));
            let rec = self.term.take_record();
            self.last = self
                .fold
                .part(&rec, BytePos::ZERO, BytePos::from_index(spelled.len()));
            self.rec = rec;
            let mut steps = WalkSteps::ZERO;
            let sniffed = self.sniff.read(&spelled, &mut steps).token_at;
            if self.cuts == Cuts::Unchecked {
                self.from = from;
                return self.last.ran;
            }
            assert_eq!(
                sniffed.map(BytePos::index),
                self.last.after().map(|(d, _)| d.index()),
                "the sniff's cut and the core's, of {:?}",
                bytes.escape_ascii().to_string()
            );
            self.from = from;
            self.last.ran
        }

        /// The last read's offset `at`, spelled, in its bytes as written.
        fn back(&self, at: BytePos) -> BytePos {
            BytePos::from_index(self.from.get(at.index()).copied().unwrap_or(at.index()))
        }

        /// **Where the last read is cut**: where the core placed its first
        /// `D` of the shell's, when in it, and what the bytes from it on
        /// say.
        pub(crate) fn cut(&self) -> Option<(BytePos, Ran)> {
            self.last.after().map(|(d, after)| (self.back(d), after))
        }

        /// The marks of the last read, at their offsets as written.
        pub(crate) fn marks(&self) -> Timeline {
            let mut t = Timeline::begun(self.last.marks.began);
            for m in self.last.marks.marks() {
                t.push(Mark {
                    at: self.back(m.at),
                    ..m
                });
            }
            t.over = self.last.marks.over.map(|o| self.back(o));
            t
        }

        /// Whether the last read carried the shell's `D`.
        pub(crate) fn shells_d(&self) -> ShellsD {
            self.last.shells_d
        }

        /// The marks folded so far, one step each.
        pub(crate) fn walked(&self) -> WalkSteps {
            self.fold.walked()
        }

        /// **The window's latch stepped over the last read**
        /// (`vouch::Latch::step`) from `latch`: the latch after it and
        /// whether the read's marks are trusted.
        pub(crate) fn latched(
            &self,
            latch: crate::vouch::Latch,
        ) -> (
            crate::vouch::Latch,
            crate::vouch::Vouch<crate::vouch::Matched>,
        ) {
            let (latch, trusted, _) = latch.step(&self.rec, &self.last.marks);
            (latch, trusted)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fed::{Cuts, Shell, with_shell, with_shell_under};
    use super::*;

    /// Marks a shell sends: `C` with `token` and `cmdline=` as `printf %q`
    /// gives a line with no special characters but blanks.
    fn c(token: &str, line: &str) -> Vec<u8> {
        let q = line.replace(' ', "\\ ");
        format!("\x1b]133;C;baud_token={token};cmdline={q}\x07").into_bytes()
    }

    fn d(token: &str) -> Vec<u8> {
        format!("\x1b]133;D;0;aid=1;baud_token={token}\x1b\\").into_bytes()
    }

    const LINK: &[u8] = b"\x1b]8;;file://localhost/etc/passwd\x1b\\x\x1b]8;;\x1b\\\r\n";

    /// **A command the shell names `ssh` makes its reads remote, from its
    /// `C` to its `D`, and `ls` does not**: the `C`, the link and the `D`
    /// in one read, and each in a read of its own; the prompt's read after
    /// the `D` is local again. Named break: ignore the mark (no `C` opens)
    /// -- the ssh's link reads local.
    #[test]
    fn a_command_named_ssh_makes_its_reads_remote_until_its_end() {
        with_shell(Some("T"), |one| {
            let mut all = c("T", "ssh localhost cat x");
            all.extend_from_slice(LINK);
            all.extend_from_slice(&d("T"));
            assert_eq!(one.read(&all), Ran::Elsewhere, "one read");
            assert_eq!(
                one.read(b"$ "),
                Ran::Unmarked,
                "the prompt after it: not ssh's"
            );
        });
        with_shell(Some("T"), |each| {
            assert_eq!(each.read(&c("T", "ssh localhost cat x")), Ran::Elsewhere);
            assert_eq!(each.read(LINK), Ran::Elsewhere, "the link");
            assert_eq!(each.read(&d("T")), Ran::Elsewhere, "the read that ends it");
            assert_eq!(each.read(b"$ "), Ran::Unmarked);
        });
        with_shell(Some("T"), |ls| {
            let mut local = c("T", "ls --hyperlink");
            local.extend_from_slice(LINK);
            local.extend_from_slice(&d("T"));
            assert_eq!(ls.read(&local), Ran::Here, "the control: ls");
        });
    }

    /// **A mark split across two reads at every cut is read whole**, its
    /// token sixteen hex digits, as baud makes one, so a cut inside it
    /// splits the value the core joins. Named break: forget what is open
    /// between parts (`Fold::part` starting from `MarkOpen::No`) -- a split
    /// mark is lost.
    #[test]
    fn a_mark_split_at_every_cut_is_read_whole() {
        let t = "0123456789abcdef";
        let m = c(t, "ssh h");
        for cut in 1..m.len() {
            with_shell(Some(t), |s| {
                let (a, b) = m.split_at(cut);
                let _ = s.read(a);
                let _ = s.read(b);
                assert_eq!(s.read(LINK), Ran::Elsewhere, "split after {cut}");
            });
        }
    }

    /// **A program on the other end cannot end its own command**: a `D`
    /// without the shell's token, or with another, a `C` naming `ls` with
    /// another token, and a fake `C` with no token all leave the ssh's
    /// reads remote; the shell's own `D` ends it. Before any token is seen
    /// a plain `D` ends it, as fish 4's own marks need. Named break: let any
    /// `D` end what is open -- the forged `D` makes the link local.
    #[test]
    fn a_mark_without_the_shells_token_lowers_nothing() {
        with_shell(Some("T"), |s| {
            let _ = s.read(&c("T", "ssh h"));
            for forged in [
                b"\x1b]133;D;0\x07".to_vec(),
                d("X"),
                c("X", "ls"),
                b"\x1b]133;C;cmdline=ls\x07".to_vec(),
                b"\x1b]133;A;aid=1\x07".to_vec(),
            ] {
                let _ = s.read(&forged);
                assert_eq!(s.read(LINK), Ran::Elsewhere, "{forged:?}");
            }
            let _ = s.read(&d("T"));
            assert_eq!(
                s.read(LINK),
                Ran::Unmarked,
                "the shell's own D: ssh's ended"
            );
        });
        with_shell(None, |fish| {
            let _ = fish.read(b"\x1b]133;C;cmdline_url=ssh%20h\x07");
            assert_eq!(fish.read(LINK), Ran::Elsewhere, "fish's own C");
            let _ = fish.read(b"\x1b]133;D;0\x07");
            assert_eq!(fish.read(LINK), Ran::Unmarked, "no token ever: a plain D");
        });
    }

    /// **A token's hash is always there**: a minted token holds sixteen hex
    /// digits, and its hash is theirs. Named break: keep the token as text
    /// and hash it only when it parses -- `hash` is an `Option` again.
    #[test]
    fn a_minted_tokens_hash_is_its_digits() {
        let t = ShellToken::mint().expect("the random source");
        let hex = baud_domain::ShellTokenHex::parse(t.text().as_bytes()).expect("sixteen digits");
        let hash: baud_domain::ShellTokenHash = t.hash();
        assert_eq!(hash, baud_domain::ShellTokenHash::of(&hex));
        assert_eq!(t.hex(), hex);
    }

    /// **The window's token is not shown**: the format of a token, alone or
    /// in an `Option`, holds none of its digits. The control: its text is
    /// them. Named break: derive `Debug` on `ShellToken` -- the digits show.
    #[test]
    fn a_shell_token_is_not_shown() {
        let digits = "0123456789abcdef";
        let token = ShellToken::given(digits);
        let shown = format!("{token:?} {:?}", Some(&token));
        assert!(!shown.contains(digits), "{shown}");
        assert_eq!(token.text(), digits, "the control");
    }

    /// **A tokened `D` fed before the core holds the token is not the
    /// shell's, and cuts nothing**: the core reads it `Absent`, the fold
    /// carries no shell's `D` and places no cut. The control: the same read
    /// once the token is handed is the shell's and cut at the `D`. Named
    /// break: read a mark's token as the shell's while the core holds none
    /// (`TokenSeen::of` giving `Shells` for no token held) -- the first read
    /// is cut.
    #[test]
    fn a_tokened_d_before_the_token_is_handed_is_not_the_shells() {
        use baud_core::{Owned, ReadLen, Term};
        let digits = "0123456789abcdef";
        let bytes = [b"x".as_slice(), &d(digits), b"% "].concat();
        let ws = baud_domain::WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let _ = t.take_record();
        let mut fold = Fold::default();
        let mut read = |t: &mut Term<'_>| {
            t.write_at(BytePos::ZERO, &bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let rec = t.take_record();
            let token = rec.marks().iter().map(|m| m.token()).next();
            let end = BytePos::from_index(bytes.len());
            (token, fold.part(&rec, BytePos::ZERO, end))
        };
        let (seen, before) = read(&mut t);
        assert_eq!(seen, Some(TokenSeen::Absent));
        assert_eq!(before.shells_d, ShellsD::No);
        assert_eq!(before.after_at(BytePos::from_index(1)), None, "no cut");
        t.set_shell_token(Some(ShellToken::given(digits).hash()));
        let (seen, after) = read(&mut t);
        assert_eq!(seen, Some(TokenSeen::Shells), "the control");
        assert_eq!(after.after_at(BytePos::from_index(1)), Some(Ran::Shell));
    }

    /// **The shell's `D` inside a picture's body ends nothing**: under the
    /// shell's `C` naming `ssh`, its `D` with the token inside a graphics
    /// command (`[[source: kitty-graphics]]`) left open (`ESC _ G a=d;`, on a
    /// terminal that ends one only at `BEL` or `ESC \\`), where the core runs
    /// no mark, and the `ST` that ends it, leave the link after them remote;
    /// the same `D` outside, the control, ends it. Named break: fold marks
    /// read from the bytes rather than from the record -- the payload's `D`
    /// ends the ssh.
    #[test]
    fn the_shells_d_in_a_pictures_body_ends_nothing() {
        for (inside, want) in [(true, Ran::Elsewhere), (false, Ran::Unmarked)] {
            let kitty = baud_domain::ControlStringEnd::Kitty;
            with_shell_under(kitty, Some("T"), Cuts::Unchecked, |s| {
                let _ = s.read(&c("T", "ssh h"));
                let d = b"\x1b]133;D;0;baud_token=T\x07";
                let d = match inside {
                    true => [b"\x1b_Ga=d;".as_slice(), d, b"\x1b\\"].concat(),
                    false => d.to_vec(),
                };
                let _ = s.read(&d);
                assert_eq!(s.read(LINK), want, "inside a picture: {inside}");
            });
        }
    }

    /// **The shell's next command ends one whose `D` never came**: a `C`
    /// for `ls` with the same token. A `C` naming another remote program
    /// keeps it open. Named break: end only at a `D` -- the window stays
    /// remote.
    #[test]
    fn the_shells_next_command_ends_one_whose_end_never_came() {
        with_shell(Some("T"), |s| {
            let _ = s.read(&c("T", "ssh h"));
            let _ = s.read(&c("T", "mosh h"));
            assert_eq!(s.read(LINK), Ran::Elsewhere, "another remote one");
            let _ = s.read(&c("T", "ls"));
            assert_eq!(s.read(LINK), Ran::Here, "the next command");
        });
    }

    /// **A `C` with no command line, or an empty one, names nothing**: one
    /// carrying the shell's token neither ends the ssh open before it nor
    /// opens a command where none is -- no `cmdline`, an empty one, `''`,
    /// an empty `cmdline_url` and an assignment alone. Named break: read a
    /// `C` that names nothing as one naming a local program -- the shell's
    /// tokened `C` ends the ssh and its link reads local.
    #[test]
    fn a_command_mark_with_no_line_names_nothing() {
        for bare in [
            b"\x1b]133;C;baud_token=T\x07".as_slice(),
            b"\x1b]133;C;baud_token=T;cmdline=\x07",
            b"\x1b]133;C;baud_token=T;cmdline=''\x07",
            b"\x1b]133;C;baud_token=T;cmdline_url=\x07",
            b"\x1b]133;C;baud_token=T;cmdline=X=1\x07",
        ] {
            with_shell(Some("T"), |open| {
                let _ = open.read(&c("T", "ssh h"));
                let _ = open.read(bare);
                assert_eq!(open.read(LINK), Ran::Elsewhere, "after an ssh: {bare:?}");
            });
            with_shell(Some("T"), |none| {
                let _ = none.read(&c("T", "ls"));
                let _ = none.read(bare);
                assert_eq!(none.read(LINK), Ran::Command, "none open: {bare:?}");
            });
        }
    }

    /// **What a read is taken as, by the shell's marks**: with no token
    /// handed to the shell, unmarked; a prompt with no `D` before it, and a
    /// key echoed at one, the samples'; a `C` alone, the command's it opens;
    /// a local command's output, the shell's local command's; the `D` that
    /// ends it, with its token, and the next prompt, the shell's own, and a
    /// key echoed after them the samples' again; output under a `C`
    /// naming nothing, a command's the samples decide; and a `C` carrying
    /// another token than the shell's, a program's, which opens nothing of
    /// the shell's. Named breaks: count the marks as output -- the `D` and
    /// the prompt read as the command's; take any token as the shell's --
    /// the forged `C` opens a local command.
    #[test]
    fn a_read_is_the_shells_once_its_command_ended() {
        with_shell(None, |s| {
            assert_eq!(s.read(b"% "), Ran::Unmarked, "no token")
        });
        with_shell(Some("T"), |s| {
            assert_eq!(s.read(b"% "), Ran::Unmarked, "a prompt with no D");
            assert_eq!(s.read(&c("T", "cat x")), Ran::Here, "the C alone");
            assert_eq!(s.read(b"x\r\n"), Ran::Here, "cat's output");
            let mut end = d("T");
            end.extend_from_slice(b"\x1b]133;A;aid=1\x07% ");
            assert_eq!(s.read(&end), Ran::Shell, "its D and the next prompt");
            assert_eq!(s.read(b"l"), Ran::Unmarked, "a key echoed");
            let _ = s.read(b"\x1b]133;C;baud_token=T\x07");
            assert_eq!(s.read(b"y\r\n"), Ran::Command, "under a C naming nothing");
            let _ = s.read(&d("T"));
            let _ = s.read(&c("X", "cat y"));
            assert_eq!(
                s.read(b"z\r\n"),
                Ran::Unmarked,
                "another token opens nothing"
            );
        });
    }

    /// **B2: a read is the shell's only if it carries the shell's `D`, and
    /// only for what follows it**: with a token handed to the shell and the
    /// shell sending no mark, as fish 4 marks its own prompts, `ssh`'s
    /// output is not the shell's; output under no command with no mark is
    /// the samples'; a `D` with the next prompt in one read stays the
    /// shell's, and output before the `D` in that read keeps its command's
    /// verdict. Named break: the shell's from no command open alone --
    /// `ssh`'s output reads as the shell's.
    #[test]
    fn a_read_is_the_shells_only_with_its_d() {
        with_shell(Some("T"), |fish4| {
            let _ = fish4.read(b"\x1b]133;A;aid=1\x07> ");
            assert_eq!(fish4.read(b"ssh h\r\n"), Ran::Unmarked, "the echo");
            assert_eq!(fish4.read(LINK), Ran::Unmarked, "ssh's output");
        });
        with_shell(Some("T"), |s| {
            assert_eq!(s.read(b"[1]+ Done\r\n"), Ran::Unmarked, "under no command");
            let _ = s.read(&c("T", "cat x"));
            let mut end = b"tail".to_vec();
            end.extend_from_slice(&d("T"));
            end.extend_from_slice(b"\x1b]133;A;aid=1\x07% ");
            assert_eq!(s.read(&end), Ran::Here, "cat's tail before its D");
            let mut prompt = d("T");
            prompt.extend_from_slice(b"% ");
            assert_eq!(s.read(&prompt), Ran::Shell, "a D and its prompt");
            assert_eq!(s.read(b"x"), Ran::Unmarked, "the next read");
        });
    }

    /// **`exec` puts another program in the shell's place, and its token
    /// goes with it**: after the shell's `C` for `exec zsh` the token is
    /// forgotten, so every read after is unmarked, and the next shell's own
    /// token, which baud never handed it, is a program's and lowers
    /// nothing. Named breaks: take `exec zsh` as a command named here -- the
    /// reads after it stay its, and a gone writer there would read local;
    /// keep the token in the core past the shell's `exec` -- the next
    /// shell's `D` with it would read as the shell's.
    #[test]
    fn exec_takes_the_shells_token_with_it() {
        with_shell(Some("T"), |s| {
            let _ = s.read(&c("T", "exec zsh"));
            assert_eq!(s.read(b"x\r\n"), Ran::Unmarked, "the shell is gone");
            let _ = s.read(&c("U", "cat y"));
            assert_eq!(s.read(b"y\r\n"), Ran::Unmarked, "a token baud never made");
            let _ = s.read(&d("U"));
            assert_eq!(s.read(b"% "), Ran::Unmarked);
            let _ = s.read(&c("T", "cat z"));
            assert_eq!(s.read(b"z\r\n"), Ran::Unmarked, "the gone shell's token");
        });
    }

    /// **B2: no token is taken from the stream**: with none handed to the
    /// shell, a remote program's tokened `C` for `ssh`, its output and its
    /// tokened `D` leave every read after them unmarked, never the
    /// shell's, so nothing they say lowers a row. Named break: adopt the
    /// first token seen -- the read after the `D` is the shell's.
    #[test]
    fn no_token_is_taken_from_the_stream() {
        with_shell(None, |s| {
            let _ = s.read(&c("x", "ssh h"));
            let _ = s.read(LINK);
            let _ = s.read(&d("x"));
            assert_eq!(s.read(b"/etc/passwd\r\n"), Ran::Unmarked, "after the D");
            let _ = s.read(&c("x", "ls"));
            assert_eq!(s.read(b"y\r\n"), Ran::Unmarked, "under a C naming ls");
        });
    }

    /// **B3: a read is the shell's only once no command is open**: under
    /// `cat x`, opened with the shell's token, a read holding only a `D`
    /// without the token, only an `A`, or ending inside a control string,
    /// is `cat`'s; the shell's own `D` then makes the next read the
    /// shell's. Named break: take a read of marks alone as the shell's --
    /// the untokened `D` reads as the shell's.
    #[test]
    fn a_read_is_the_shells_only_once_no_command_is_open() {
        with_shell(Some("T"), |s| {
            let _ = s.read(&c("T", "cat x"));
            assert_eq!(
                s.read(b"\x1b]133;D;0\x07"),
                Ran::Here,
                "a D without the token"
            );
            assert_eq!(s.read(b"\x1b]133;A;aid=1\x07"), Ran::Here, "an A alone");
            assert_eq!(s.read(b"\x1b]0;title"), Ran::Here, "a body left open");
            assert_eq!(s.read(b"\x07"), Ran::Here, "its end");
            let mut prompt = d("T");
            prompt.extend_from_slice(b"% ");
            assert_eq!(s.read(&prompt), Ran::Shell, "the shell's own D");
        });
    }

    /// **B2's guards, each on its own**: with nothing open, a read of marks
    /// alone -- fish 4's `A`, or a `D` with another token -- is unmarked;
    /// output before the shell's `D` keeps the whole read unmarked; a `D`
    /// is the shell's only with its token, and only then is the link after
    /// it the shell's. Named breaks, one an assert: the shell's from marks
    /// alone under no command (`Out::ran`'s `MarkOpen::No` arm without its
    /// `shells_d` test) -- the `A` alone and the foreign `D` read as the
    /// shell's; output under no command counted as marks (`Out::saw`'s
    /// `ShellsD::No` arm as `Out::Nothing`) -- the job's line and the
    /// prompt read as the shell's; any token taken as the shell's in the
    /// core (`TokenSeen::of`) -- the foreign `D` and its link read as the
    /// shell's.
    #[test]
    fn b2s_guards_each_hold_alone() {
        with_shell(Some("T"), |s| {
            assert_eq!(s.read(b"\x1b]133;A;aid=1\x07"), Ran::Unmarked, "an A alone");
            assert_eq!(s.read(&d("X")), Ran::Unmarked, "a foreign D alone");
            let mut done = b"[1]+ Done\r\n".to_vec();
            done.extend_from_slice(&d("T"));
            done.extend_from_slice(b"% ");
            assert_eq!(s.read(&done), Ran::Unmarked, "a job's line, then the D");
            let mut forged = d("X");
            forged.extend_from_slice(LINK);
            assert_eq!(s.read(&forged), Ran::Unmarked, "a foreign D, then a link");
            let mut genuine = d("T");
            genuine.extend_from_slice(LINK);
            assert_eq!(s.read(&genuine), Ran::Shell, "the shell's D, then a link");
        });
    }

    /// **A `C` after the shell's `D` in one read is its command's**: one
    /// naming `cat` is here, one naming nothing the samples'. Named break:
    /// the shell's whenever the read carried its `D`, whatever is open
    /// after it -- both read as the shell's.
    #[test]
    fn a_command_after_the_shells_d_is_its_own() {
        with_shell(Some("T"), |s| {
            let mut named = d("T");
            named.extend_from_slice(&c("T", "cat x"));
            assert_eq!(s.read(&named), Ran::Here, "a C naming cat");
            let mut unnamed = d("T");
            unnamed.extend_from_slice(b"\x1b]133;C;baud_token=T\x07");
            assert_eq!(s.read(&unnamed), Ran::Command, "a C naming nothing");
        });
    }

    /// **A read is cut only at the shell's own `D`, begun in it**: under
    /// `ssh h`, ssh's last line, the shell's `D` and its prompt are cut at
    /// the `D`, the bytes from it the shell's; a `D` without the token, or
    /// with another, cuts nothing, and neither does the shell's `D` begun
    /// in the read before; of two of the shell's `D`s the first cuts.
    /// Named break: the cut taken at any `D` -- the forged ones cut.
    #[test]
    fn a_read_is_cut_only_at_the_shells_own_d() {
        let at = |s: &Shell<'_>| s.cut().map(|(d, after)| (d.index(), after));
        with_shell(Some("T"), |s| {
            let _ = s.read(&c("T", "ssh h"));
            let end = [b"bye\r\n".as_slice(), &d("T"), b"% "].concat();
            assert_eq!(s.read(&end), Ran::Elsewhere, "the read uncut");
            assert_eq!(at(s), Some((5, Ran::Shell)), "cut at the shell's D");
            let _ = s.read(&c("T", "ssh h"));
            let untokened = b"x\x1b]133;D;0\x07% ".to_vec();
            let foreign = [b"x".as_slice(), &d("X"), b"% "].concat();
            for forged in [untokened, foreign] {
                assert_eq!(s.read(&forged), Ran::Elsewhere);
                assert_eq!(at(s), None, "{:?}", forged.escape_ascii().to_string());
            }
            let whole = d("T");
            let _ = s.read(&whole[..3]);
            let _ = s.read(&[&whole[3..], b"% ".as_slice()].concat());
            assert_eq!(at(s), None, "split between two reads");
            let _ = s.read(&c("T", "ssh h"));
            let two = [b"y".as_slice(), &d("T"), b"% ", &d("T"), b"% "].concat();
            let _ = s.read(&two);
            assert_eq!(at(s), Some((1, Ran::Shell)), "the first of two");
        });
    }

    /// **With no token, a command naming a program here ends a remote
    /// one**, as fish 4's own marks need, which carry none. Named break:
    /// keep what is open on a `C` with no token.
    #[test]
    fn with_no_token_a_local_command_ends_a_remote_one() {
        with_shell(None, |s| {
            let _ = s.read(b"\x1b]133;C;cmdline_url=ssh%20h\x07");
            assert_eq!(s.read(LINK), Ran::Elsewhere, "ssh's");
            let _ = s.read(b"\x1b]133;C;cmdline_url=ls\x07");
            assert_eq!(s.read(LINK), Ran::Unmarked, "ended by the next");
        });
    }

    /// **A command line longer than the core keeps of a control string
    /// still opens its command in the core's prompt log, ends it with its
    /// status, and opens the fold**: the core clips the `C` at `OSC_MAX` and
    /// still reads it as a `C` (`Term::prompts`), as it does a short one,
    /// the control; the token and the first word come first, so the core
    /// reads them from the part it keeps. Named break: name nothing from a
    /// cut command line (`CommandRuns::of_clipped` giving `Nothing`) -- the
    /// long command opens nothing.
    #[test]
    fn a_long_command_line_still_reaches_the_cores_marks() {
        use baud_domain::{ExitCode, ExitStatus};
        for words in [1, 2000] {
            let line = format!("ssh{}", "\\ x".repeat(words));
            let c = format!("\x1b]133;C;baud_token=t;cmdline={line}\x07");
            with_shell(Some("t"), |s| {
                let bytes = format!(
                    "\x1b]133;A\x07$ \x1b]133;B\x07\r\n{c}out\r\n\x1b]133;D;3;baud_token=t\x07\x1b]133;A\x07$ "
                );
                let _ = s.read(bytes.as_bytes());
                let statuses: Vec<ExitStatus> = s.term.prompts().map(|p| p.status).collect();
                assert!(
                    statuses.contains(&ExitStatus::Exited(ExitCode::of(3))),
                    "{} bytes of C: {statuses:?}",
                    c.len()
                );
            });
            with_shell(Some("t"), |s| {
                assert_eq!(s.read(c.as_bytes()), Ran::Elsewhere, "{} bytes", c.len());
                assert_eq!(s.read(b"\x1b]133;D;3;baud_token=t\x07"), Ran::Elsewhere);
                assert_eq!(s.read(b"$ "), Ran::Unmarked, "closed by the shell's D");
            });
        }
    }

    /// **A program word cut at the bound names nothing**: a `C` whose
    /// program path runs past `OSC_MAX`, `/usr/a...a/ssh`, ends no remote
    /// command, where its cut word, `a...a`, would read as a program here
    /// and end the `ssh` the same shell had opened. The control: a short
    /// local path ends it. Named break: name the cut word.
    #[test]
    fn a_cut_program_word_names_nothing() {
        for (path, closed) in [
            (String::from("/usr/a/ls"), true),
            (
                format!("/usr/{}/ssh", "a".repeat(baud_core::OSC_MAX)),
                false,
            ),
        ] {
            with_shell(Some("t"), |scan| {
                let open = scan.read(b"\x1b]133;C;baud_token=t;cmdline=ssh\x07");
                assert_eq!(open, Ran::Elsewhere, "opened");
                let c = format!("\x1b]133;C;baud_token=t;cmdline={path}\x07");
                scan.read(c.as_bytes());
                let after = match closed {
                    true => Ran::Here,
                    false => Ran::Elsewhere,
                };
                assert_eq!(scan.read(b"$ "), after, "{} bytes of C", c.len());
            });
        }
    }

    /// **The fold keeps each mark at its terminator, where the core
    /// recorded it**: `ESC ] 133 ; A BEL` ends at its `BEL`, and
    /// `ESC ] 133 ; D ESC \` at its `\`; a mark with no letter is not
    /// recorded; and the shell's `D` with its token is told from a `D`
    /// without one. Named break: keep a mark at the offset past its
    /// terminator -- every offset is one more.
    #[test]
    fn the_fold_keeps_each_mark_where_the_core_recorded_it() {
        let bytes = b"\x1b]133;A\x07$ \x1b]133;\x07\x1b]133;D;0;baud_token=t\x1b\\\x1b]133;D\x07";
        with_shell(Some("t"), |s| {
            let _ = s.read(bytes);
            let got: Vec<(usize, u8, ShellsD)> = s
                .marks()
                .marks()
                .map(|m| (m.at().index(), m.letter().raw_byte(), m.shells_d()))
                .collect();
            let bel = |from: usize| from + bytes[from..].iter().position(|&b| b == 0x07).unwrap();
            let st = 1 + bytes.windows(2).position(|w| w == b"\x1b\\").unwrap();
            assert_eq!(
                got,
                [
                    (bel(0), b'A', ShellsD::No),
                    (st, b'D', ShellsD::Yes),
                    (bel(st), b'D', ShellsD::No),
                ]
            );
        });
    }

    /// **The shell's `D` with its token closes whatever was open**: from
    /// every state a window with a token can hold -- nothing open, a
    /// remote command opened without the token or with it, one named here,
    /// one naming nothing -- the `D` carrying it leaves nothing open and is
    /// the shell's. What a re-sync rests on (`vouch::Resync`): after it
    /// what is open depends on nothing before. Named break: a remote
    /// command opened without the token ends only at a `D` without one
    /// (`MarkOpen::ends`) -- it stays open.
    #[test]
    fn the_shells_d_closes_every_state() {
        use baud_domain::{CommandRuns, TokenSeen};
        let d = StreamByte::from_stream(b'D');
        for open in [
            MarkOpen::No,
            MarkOpen::Remote(TokenSeen::Other),
            MarkOpen::Remote(TokenSeen::Shells),
            MarkOpen::Here(TokenSeen::Shells),
            MarkOpen::Unnamed(TokenSeen::Shells),
        ] {
            assert_eq!(
                open.step(d, TokenSeen::Shells, CommandRuns::Nothing),
                MarkOpen::No,
                "{open:?}"
            );
        }
        with_shell(Some("t"), |s| {
            let _ = s.read(b"\x1b]133;C;cmdline=ssh\x07");
            let _ = s.read(b"\x1b]133;D;0;baud_token=t\x07");
            assert_eq!(s.shells_d(), ShellsD::Yes);
            assert_eq!(s.marks().open_at(BytePos::from_index(99)), Left::Closed);
        });
    }

    /// **A part's marks match the core's only offset for offset and letter
    /// for letter** (`vouch::Matched`): the fold's marks of a part match
    /// the core's record of it; moved a byte either way, or with another
    /// letter, they do not; and past the kept marks they do not. Named
    /// breaks: compare letters alone -- the moved marks match; compare
    /// offsets alone -- the other letter matches.
    #[test]
    fn marks_match_only_where_and_as_the_core_ran_them() {
        use crate::vouch::{Matched, Vouch, Why};
        use baud_core::{Owned, ReadLen, Term};
        let ws = baud_domain::WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let bytes = b"\x1b]133;A\x07$ \x1b]133;D;0;baud_token=0123456789abcdef\x1b\\";
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let _ = t.take_record();
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        let rec = t.take_record();
        let ran = Fold::default()
            .part(&rec, BytePos::ZERO, BytePos::from_index(bytes.len()))
            .marks;
        assert!(matches!(Matched::of(&rec, &ran), Vouch::Yes(_)), "as read");
        let changed = |f: &dyn Fn(Mark) -> Mark| {
            let mut t = Timeline::begun(ran.began);
            ran.marks().for_each(|m| t.push(f(m)));
            t
        };
        for (what, moved) in [
            (
                "a byte later",
                changed(&|m| Mark {
                    at: BytePos::from_index(m.at.index() + 1),
                    ..m
                }),
            ),
            (
                "a byte sooner",
                changed(&|m| Mark {
                    at: BytePos::from_index(m.at.index() - 1),
                    ..m
                }),
            ),
            (
                "another letter",
                changed(&|m| Mark {
                    letter: StreamByte::from_stream(m.letter.raw_byte() + 1),
                    ..m
                }),
            ),
        ] {
            assert_eq!(Matched::of(&rec, &moved), Vouch::No(Why::Marks), "{what}");
        }
        let mut over = ran;
        over.over = Some(BytePos::ZERO);
        assert_eq!(
            Matched::of(&rec, &over),
            Vouch::No(Why::Marks),
            "past the kept"
        );
    }

    /// **A part with more marks than the record keeps is overflowed, and
    /// every part past it is untrusted**: nine `A`s in one read fold eight
    /// and say the part overflowed, so its marks match nothing; eight do
    /// not. The next read, one `A` and a prompt, keeps and matches its own
    /// marks, and past the nine it still vouches nothing: the window's
    /// latch drifted at the overflowed part, and no `D` of the shell's has
    /// re-synced it (`vouch::Latch`); past the eight it is trusted, the
    /// control. Named break: fold the kept marks as the whole part (no
    /// `over` on `Fill::Overflowed`) -- the nine match.
    #[test]
    fn a_part_past_the_records_marks_is_overflowed() {
        use crate::vouch::{Latch, Vouch, Why};
        let why = |v: Vouch<crate::vouch::Matched>| match v {
            Vouch::Yes(_) => None,
            Vouch::No(why) => Some(why),
        };
        for (n, over) in [(8, false), (9, true)] {
            with_shell(None, |s| {
                let _ = s.read(&b"\x1b]133;A\x07".repeat(n));
                assert_eq!(s.marks().overflowed(), over, "{n} marks");
                assert_eq!(s.marks().marks().count(), 8.min(n));
                let (latch, trusted) = s.latched(Latch::Synced);
                assert_eq!(why(trusted), over.then_some(Why::Marks), "{n} marks");
                let _ = s.read(b"\x1b]133;A\x07$ ");
                assert!(!s.marks().overflowed(), "past {n}: its own kept");
                let (_, past) = s.latched(latch);
                assert_eq!(why(past), over.then_some(Why::Drifted), "past {n}");
            });
        }
    }

    /// **What was open where a folder report came is what the fold read
    /// there**: after a command the shell named here, its `D`, `A` and an
    /// `OSC 7` in one read come with nothing open, the read the shell's;
    /// under `ssh`, a report before the shell's `D` comes under the remote
    /// command and one after it, in the same read, with nothing open.
    /// Named break: judge every report by what was open as the read began
    /// (`Timeline::open_at` returning `began`) -- the shell's report after
    /// `ssh`'s `D` reads remote.
    #[test]
    fn a_folder_report_is_judged_by_what_ran_as_it_came() {
        let at7 = |bytes: &[u8], from: usize| {
            BytePos::from_index(from + bytes[from..].windows(4).position(|w| w == b"]7;f").unwrap())
        };
        with_shell(Some("t"), |scan| {
            let _ = scan.read(b"\x1b]133;C;baud_token=t;cmdline=ls\x07out\r\n");
            let prompt = b"\x1b]133;D;0;baud_token=t\x07\x1b]133;A\x07\x1b]7;file://h/tmp\x07% ";
            assert_eq!(scan.read(prompt), Ran::Shell, "the prompt's read");
            assert_eq!(scan.marks().open_at(at7(prompt, 0)), Left::Closed);
        });
        with_shell(Some("t"), |scan| {
            let _ = scan.read(b"\x1b]133;C;baud_token=t;cmdline=ssh\\ h\x07");
            let shells = b"\x1b]7;file://h/tmp\x07\x1b]133;D;0;baud_token=t\x07\x1b]133;A\x07\x1b]7;file://h/home\x07% ";
            assert_eq!(scan.read(shells), Ran::Elsewhere, "the read of ssh's D");
            assert_eq!(
                scan.marks().open_at(at7(shells, 0)),
                Left::Remote,
                "the remote program's"
            );
            assert_eq!(
                scan.marks().open_at(at7(shells, 5)),
                Left::Closed,
                "the shell's, after its D"
            );
        });
    }

    /// **The sniff cuts zsh's `PROMPT_SP` reads where the core does**: the
    /// cases `cut::tests::zshs_run_is_read_exactly` cuts, each ended by the
    /// shell's `D` with its token, are cut at the `D`, by the core's record
    /// and by the sniff ([`with_shell`] checks each read). Named break: the
    /// fold placing the cut at the `D`'s terminator -- every case fails.
    #[test]
    fn the_sniff_cuts_zshs_runs_where_the_core_does() {
        let run = |mark: &str, blanks: usize, tail: &str| {
            format!(
                "\x1b[1m\x1b[7m{mark}\x1b[27m\x1b[1m\x1b[0m{}\r{tail}",
                " ".repeat(blanks)
            )
        };
        for bytes in [
            format!("logout\r\n{}", run("%", 79, " \r")),
            run("#", 3, "\r"),
            format!("remote$ {}", run("%", 79, " \r")),
            run("\u{bb}", 79, " \r"),
            run("%", 0, " \r"),
            format!("{}\n", run("%", 79, " \r")),
        ] {
            with_shell(Some("T"), |s| {
                let _ = s.read(&c("T", "ssh h"));
                let read = [bytes.as_bytes(), &d("T"), b"% "].concat();
                assert_eq!(s.read(&read), Ran::Elsewhere, "{bytes:?}");
                assert_eq!(
                    s.cut().map(|(at, _)| at.index()),
                    Some(bytes.len()),
                    "{bytes:?}"
                );
            });
        }
    }

    /// **On generated streams of marks, read in any sizes, the sniff cuts
    /// every read where the core does**: 20,000 streams of up to twelve
    /// pieces -- the shell's `C` and `D`, either with another token or with
    /// none, a `D` with an empty token option first, with another token's
    /// option first, with its token past or inside a body's first 512 bytes,
    /// one a `CAN` cancels, `ESC ESC ]`, a title `OSC` its `ESC` abandons,
    /// links and text -- each cut into reads of 1 to 40 bytes, or now and
    /// then 200 ([`with_shell`] checks each read). An `exec` that replaces
    /// the shell is not generated: the core forgets the token there, and
    /// the sniff does not. Named break: the fold placing the cut at a `D`
    /// that began in the read before (`DAt::At` for a mark begun before the
    /// part) -- the reads that end a split `D` fail.
    #[test]
    fn the_sniffs_cut_is_the_cores_on_generated_marks() {
        // Tokens of sixteen digits, so a value split across reads is the
        // sniff's to join.
        let (t, x) = ("0123456789abcdef", "fedcba9876543210");
        let pad = format!("pad={}", "x".repeat(600));
        let pieces: Vec<Vec<u8>> = vec![
            c(t, "ssh h"),
            c(t, "ls"),
            c(x, "ls"),
            d(t),
            d(x),
            b"\x1b]133;D;0\x07".to_vec(),
            format!("\x1b]133;D;0;baud_token=;baud_token={t}\x07").into_bytes(),
            format!("\x1b]133;D;0;baud_token={x};baud_token={t}\x07").into_bytes(),
            format!("\x1b]133;D;0;baud_token={t};{pad}\x07").into_bytes(),
            format!("\x1b]133;D;0;{pad};baud_token={t};aid=1\x07").into_bytes(),
            format!("\x1b]133;D;0;baud_token={t}\x18\x07").into_bytes(),
            b"\x1b".to_vec(),
            b"\x1b]0;title".to_vec(),
            LINK.to_vec(),
            b"out\r\n".to_vec(),
            b"% ".to_vec(),
        ];
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = |n: usize| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            usize::try_from(seed >> 33).expect("31 bits") % n
        };
        let mut cut = 0;
        for _ in 0..20_000 {
            let count = 1 + next(12);
            let stream: Vec<u8> = (0..count)
                .flat_map(|_| pieces[next(pieces.len())].clone())
                .collect();
            with_shell(Some(t), |s| {
                let mut at = 0;
                while at < stream.len() {
                    let most = match next(4) {
                        0 => 200,
                        _ => 40,
                    };
                    let to = (at + 1 + next(most)).min(stream.len());
                    let _ = s.read(&stream[at..to]);
                    cut += usize::from(s.cut().is_some());
                    at = to;
                }
            });
        }
        assert!(cut > 1000, "{cut} reads cut");
    }

    /// **A read with no mark walks nothing for marks, and a mark is one
    /// step**: 4,096 bytes with an `SGR` every 17 fold no step and read no
    /// command line in the core; the same with the shell's `C` after them
    /// fold one step and read one command line, `ELSEWHERE` walked once.
    /// Named break: fold a step a byte -- the read with no mark walks
    /// 4,096.
    #[test]
    fn a_read_with_no_mark_walks_no_byte_for_marks() {
        let sgr: Vec<u8> = b"\x1b[1mabcdefghijkl"
            .iter()
            .copied()
            .cycle()
            .take(4096)
            .collect();
        with_shell(Some("T"), |s| {
            let lines = s.term.command_lines_read().get();
            let _ = s.read(&sgr);
            assert_eq!(s.walked().get(), 0, "marks_bytes_walked");
            assert_eq!(s.term.command_lines_read().get() - lines, 0);
            let _ = s.read(&[sgr.as_slice(), &c("T", "ssh h")].concat());
            assert_eq!(s.walked().get(), 1, "the control: one mark");
            assert_eq!(s.term.command_lines_read().get() - lines, 1);
        });
    }
}
