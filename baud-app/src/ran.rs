//! **What the shell says it runs**, read off the bytes as the reader takes
//! them: `OSC 133 ; C` with kitty's `cmdline=` or `cmdline_url=`
//! (`[[source: kitty-shell-integration]]`, "kitty also optionally supports
//! sending the cmdline going to be executed with `<OSC>133;C`"; fish sends
//! the second form itself, `[[source: fish-terminal-compat]]`), and the
//! `OSC 133 ; D` that ends the command.
//!
//! A command whose program is one not on this machine -- ssh and its kin,
//! the paths worker's own list (`probe::front_of`) -- makes every read
//! from its `C` to its `D` remote, for a link's origin and a printed
//! path's mark, on those reads' rows. The mark comes in the stream before
//! the command's first byte, so a job that starts, prints a link and ends
//! between two samples of the group in front is still named, on both
//! systems and with no call to the kernel. Each read is taken as one
//! [`Ran`]: the remote command's, a local command's, or the shell's own --
//! what follows the shell's `D`, with its token, its prompt after a
//! command, which the program sampled before it did not write; any other
//! read under no command is the samples' to decide.
//!
//! **A mark can only raise.** A remote program can print marks too, so
//! nothing in a mark lowers what the reads are taken as unless it carries
//! the token baud made for the window's shell ([`ShellToken`]), handed to
//! its integration at the spawn and never to anything after: `baud_token=`
//! on its `C` and its `D`. [`Commands`] is built with it and never takes
//! one from the stream, so a mark with any other is a program's. A program
//! on the other end cannot end the command early with a `D` of its own,
//! and a `C` of the shell's naming a program here lowers only a writer
//! gone before it was named (`paths::Paths::written_here`), never one
//! named not local. With no token -- the integration off, a shell marking
//! its own prompts, as fish 4 does, or another integration -- no read is
//! the shell's or a command's named here, so nothing lowers; a plain `D`
//! ends a remote command, and the samples and the process watch are what
//! is left against a program that prints one.
//!
//! **A `C` with no command line, or an empty one, names nothing**
//! ([`Named::Nothing`]): it neither opens a remote command nor ends one,
//! and the samples and the process watch decide. bash sends one when its
//! history kept no line for the command, as `history 1` is then another
//! command's; zsh when its history mechanism is not active.
//!
//! It is scanned on the thread taking the reads, as the `OSC 8`
//! introducer is (`probe::Sampler`), and runs beside the core's own reading
//! of the same marks, which keeps none of these options. **The core's
//! reading decides**: each part's marks are checked against the core's
//! record of it, offset for offset and letter for letter
//! (`vouch::Matched`), and from a part where they differ the window's marks
//! are not trusted until the shell's `D` with its token re-syncs them
//! (`vouch::Latch`), since this scan's state carries from read to read and
//! the reader's thread never learns the core disagreed.

use baud_domain::{BytePos, StreamByte};

/// The longest mark body kept: the core keeps no more of a control string
/// (`baud_core::OSC_MAX`), and the token and the command's first word come
/// first in it.
const BODY: usize = 512;

/// Where the scan is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum At {
    #[default]
    Text,
    /// After an `ESC` in text.
    Esc,
    /// Inside an `OSC` body.
    Body,
    /// After an `ESC` inside a body: `\` ends it, anything else abandons
    /// it and starts an escape sequence of its own.
    BodyEsc,
}

/// **The token baud makes for one window's shell**: sixteen hex digits of
/// the system's random source, handed to the integration at the spawn
/// (`shell::inject`), which takes it out of the environment before the
/// user's startup files run and sends it on its own `C` and `D` marks. No
/// program on the other end of an ssh can know it. Made only by
/// [`ShellToken::mint`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ShellToken(String);

impl ShellToken {
    /// A new token, none when the random source cannot be read: the window's
    /// shell is then marked with none, and nothing it marks lowers.
    pub fn mint() -> Option<ShellToken> {
        use std::io::Read;
        let mut raw = [0u8; 8];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut raw))
            .ok()?;
        Some(ShellToken(raw.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// Its text, as the integration sends it.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A token spelled `s`, for a test.
    #[cfg(test)]
    pub fn given(s: &str) -> ShellToken {
        ShellToken(s.to_string())
    }
}

/// A shell's token, kept as a hash of its bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Token(u64);

impl Token {
    /// FNV-1a over the value's bytes.
    fn of(s: &[u8]) -> Token {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in s {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Token(h)
    }
}

/// **What a `C` names**: only [`Named::Elsewhere`] opens a remote command,
/// and only [`Named::Here`] may end one, so a `C` that names nothing is
/// never read as either.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Named {
    /// No command line, or one with no program in it: the samples and the
    /// process watch decide.
    Nothing,
    /// A program on this machine.
    Here,
    /// A program the paths worker names not local.
    Elsewhere,
    /// A program on this machine `exec` puts in the shell's place: the
    /// shell and its token end with it, and the next shell, if it marks
    /// its prompts, brings its own.
    Replaced,
}

impl Named {
    /// What the `C` mark with `fields` names: the command line of the
    /// first `cmdline` or `cmdline_url` field, cut where `tail` says the
    /// last field was.
    fn of(fields: &[u8], tail: Tail) -> Named {
        let cut_from = match tail {
            Tail::Cut => fields.iter().rposition(|&b| b == b';').map_or(0, |i| i + 1),
            Tail::Whole => fields.len() + 1,
        };
        let mut at = 0;
        for f in fields.split(|&b| b == b';') {
            let line_tail = match at >= cut_from {
                true => Tail::Cut,
                false => Tail::Whole,
            };
            at += f.len() + 1;
            if let Some(line) = field_line(f) {
                return names(&line, line_tail);
            }
        }
        Named::Nothing
    }
}

/// **Whether a body, or the command line read from it, reached the bound
/// and was cut there**: a word that runs to a cut end is not the word
/// sent, and names nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tail {
    Whole,
    Cut,
}

/// **Which command the shell said is running**, and the token of the shell
/// that said so: one named not local, with the token if its `C` carried
/// one; one named on this machine, or naming nothing, which only the
/// shell's own token opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Open {
    #[default]
    No,
    Remote(Option<Token>),
    Here(Token),
    Unnamed(Token),
}

/// **What the shell's marks say of a read** ([`Commands::read`]), which the
/// paths file into the read's own rows (`probe::Probes::read_taken`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Ran {
    /// No token for the window's shell, or none since `exec` took it: the
    /// samples and the process watch decide.
    #[default]
    Unmarked,
    /// The shell's own: a read carrying the shell's `D`, with its token,
    /// and nothing open after it, and of its bytes only those after that
    /// mark -- the prompt after a command. What the group sampled before
    /// the read wrote is not in it. A read with no such mark is not the
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
    /// ran -- the read of its `C` and of its `D` included.
    Elsewhere,
    /// Written while a command whose `C` named nothing ran: the samples and
    /// the process watch decide.
    Command,
}

/// How much a [`Ran`] lets a read's rows be lowered, least first: what
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

    /// **The more cautious of two reads' verdicts**, for reads filed over
    /// one span, as a drain of them is on macOS (`probe::Probes::drain_taken`):
    /// a command named not local over one named nothing, over one named
    /// here, over the shell's own, so no read's verdict lowers the rows
    /// another read in the span wrote. The shell's only if every read is.
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
/// `D`, in the same read as the `D` and the prompt, is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Folder {
    #[default]
    Unreported,
    Reported,
    FromRemote,
}

/// **What was open after a mark**, as [`Commands`] read it: nothing, a
/// command the shell named here or naming nothing, or one named not local.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Left {
    #[default]
    Closed,
    Here,
    Remote,
}

impl Left {
    fn of(open: Open) -> Left {
        match open {
            Open::No => Left::Closed,
            Open::Here(_) | Open::Unnamed(_) => Left::Here,
            Open::Remote(_) => Left::Remote,
        }
    }
}

/// **One `OSC 133` mark the scan read**: where its terminator is in the
/// read -- the offset the core records its own at
/// (`baud_core::MarkEvent::at`) -- its letter, the first byte after
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
/// part (`baud_core::ReadRecord::marks`).
const MARKS: usize = 8;

/// **The marks a read carried, where each ended, and what was open as it
/// began**, so a part of the read fed while the rest is held files only
/// the marks it fed (`reads::Part`), and each is checked against the
/// core's record of the same part offset for offset and letter for letter
/// (`vouch::Matched`). Eight are kept; past them the scan says where the
/// first it dropped ended, and every part reaching past it is unmatched.
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

    /// The marks, in the order the read ended them.
    pub fn marks(&self) -> impl Iterator<Item = Mark> + '_ {
        self.kept.iter().map_while(|m| *m)
    }

    /// Whether the read held marks past the kept ones before `to`.
    pub fn overflowed(&self) -> bool {
        self.over.is_some()
    }

    /// **What was open at `at`**: after the last mark that ended before
    /// it, or as the read began.
    pub fn open_at(&self, at: BytePos) -> Left {
        self.marks()
            .filter(|m| m.at < at)
            .last()
            .map_or(self.began, |m| m.left)
    }

    /// **The marks of the bytes from `from` to `to`**, a mark belonging to
    /// the part that fed its terminator; overflowed if the part reaches
    /// past the first mark dropped.
    pub fn part(&self, from: BytePos, to: BytePos) -> Timeline {
        let mut part = Timeline::begun(self.open_at(from));
        self.marks()
            .filter(|m| from <= m.at && m.at < to)
            .for_each(|m| part.push(m));
        part.over = self.over.filter(|&o| o < to);
        part
    }

    /// **The marks of the bytes from `from` on**, counted from there.
    pub fn rest(&self, from: BytePos) -> Timeline {
        let mut rest = Timeline::begun(self.open_at(from));
        let back = |e: BytePos| BytePos::from_index(e.index().saturating_sub(from.index()));
        self.marks().filter(|m| m.at >= from).for_each(|m| {
            rest.push(Mark {
                at: back(m.at),
                ..m
            })
        });
        rest.over = self.over.map(back);
        rest
    }
}

/// Whether a read carried the shell's `D`, with its token.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShellsD {
    No,
    Yes,
}

/// **The scan of the reads for the shell's command marks**: one per
/// reader, carried across reads, so a mark split between two is read
/// whole.
#[derive(Clone, Debug)]
pub struct Commands {
    at: At,
    /// The body read so far, at most [`BODY`] bytes of it.
    body: Vec<u8>,
    open: Open,
    /// The window's shell's token, as baud made it: set once, at the start,
    /// and forgotten when `exec` puts another program in the shell's place;
    /// never taken from the stream.
    known: Option<Token>,
    /// The marks the last read carried ([`Timeline`]).
    marks: Timeline,
    /// Whether the read so far carried the shell's `D`, with its token.
    shells_d: ShellsD,
    /// Where in the read the sequence the scan is in began, if in it.
    esc_at: Option<Byte>,
    /// Where an `ESC` inside a body was, if in the read: the sequence it
    /// begins when it is not `ST`.
    body_esc_at: Option<Byte>,
    /// Where the read's first `D` of the shell's began.
    d: DAt,
    /// The last read's cut ([`Commands::cut`]).
    cut: Option<(Byte, Ran)>,
}

/// A byte's place in a read, shown as its offset.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Byte(BytePos);

impl core::fmt::Debug for Byte {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0.index())
    }
}

/// Where a read's first `D` of the shell's, with its token, began.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DAt {
    Unseen,
    At(Byte),
    /// In an earlier read: the `D` was split between two.
    Earlier,
}

/// **What a read was written under, from its start and from its shell's
/// `D` on**: the second only once that `D` came.
#[derive(Clone, Copy, Default, Debug)]
struct Outs {
    all: Out,
    after: Option<Out>,
}

impl Outs {
    fn saw(&mut self, open: Open, what: Wrote, shells_d: ShellsD) {
        self.all.saw(open, what, shells_d);
        if let Some(after) = self.after.as_mut() {
            after.saw(open, what, shells_d);
        }
    }
}

/// With no token: how a shell the integration is off for is read.
impl Default for Commands {
    fn default() -> Commands {
        Commands::new(None)
    }
}

impl Commands {
    /// **The scan for a window whose shell baud handed `token`**, the only
    /// token any mark is read against.
    pub fn new(token: Option<&ShellToken>) -> Commands {
        Commands {
            at: At::Text,
            body: Vec::with_capacity(BODY),
            open: Open::No,
            known: token.map(|t| Token::of(t.as_str().as_bytes())),
            marks: Timeline::default(),
            shells_d: ShellsD::No,
            esc_at: None,
            body_esc_at: None,
            d: DAt::Unseen,
            cut: None,
        }
    }

    /// **What the shell's marks say of `bytes`** ([`Ran`]): written at any
    /// point while a command the shell named not local ran -- open as the
    /// read began, or opened by a mark inside it, as a short ssh's `C`, its
    /// link and its `D` can all be one read; else what wrote the bytes that
    /// are not the shell's marks: a command the shell named here, one that
    /// named nothing, or none: the shell's own only after its `D`, with its
    /// token, and with no command open after it ([`Ran::Shell`]); any other
    /// read under none is unmarked, the samples'.
    pub fn read(&mut self, bytes: &[u8]) -> Ran {
        self.marks = Timeline::begun(Left::of(self.open));
        self.shells_d = ShellsD::No;
        self.esc_at = None;
        self.body_esc_at = None;
        self.d = DAt::Unseen;
        let mut out = Outs::default();
        out.saw(self.open, Wrote::Marks, self.shells_d);
        let mut i = 0;
        while i < bytes.len() {
            match self.at {
                At::Text => {
                    let p = bytes[i..].iter().position(|&b| b == 0x1b);
                    if p != Some(0) {
                        out.saw(self.open, Wrote::Output, self.shells_d);
                    }
                    match p {
                        Some(p) => {
                            self.esc_at = Some(Byte(BytePos::from_index(i + p)));
                            i += p + 1;
                            self.at = At::Esc;
                        }
                        None => break,
                    }
                }
                At::Esc => {
                    if bytes[i] == b']' {
                        i += 1;
                        self.body.clear();
                        self.at = At::Body;
                    } else if bytes[i] != 0x1b {
                        // An escape sequence of the program's own.
                        out.saw(self.open, Wrote::Output, self.shells_d);
                        self.at = At::Text;
                    } else {
                        self.esc_at = Some(Byte(BytePos::from_index(i)));
                        i += 1;
                    }
                }
                At::Body => {
                    // The run up to the next byte that ends the body, kept
                    // to `BODY`, in one step: a long `OSC 52` costs a search.
                    let rest = &bytes[i..];
                    let run = rest
                        .iter()
                        .position(|&b| matches!(b, 0x07 | 0x1b | 0x18 | 0x1a))
                        .unwrap_or(rest.len());
                    let room = BODY - self.body.len().min(BODY);
                    self.body.extend_from_slice(&rest[..run.min(room)]);
                    i += run;
                    let Some(&b) = bytes.get(i) else {
                        break;
                    };
                    i += 1;
                    match b {
                        0x07 => {
                            self.at = At::Text;
                            self.ended(&mut out, BytePos::from_index(i));
                        }
                        0x1b => {
                            self.body_esc_at =
                                i.checked_sub(1).map(|e| Byte(BytePos::from_index(e)));
                            self.at = At::BodyEsc;
                        }
                        // CAN and SUB abandon it.
                        _ => {
                            out.saw(self.open, Wrote::Output, self.shells_d);
                            self.at = At::Text;
                        }
                    }
                }
                At::BodyEsc => {
                    if bytes[i] == b'\\' {
                        i += 1;
                        self.at = At::Text;
                        self.ended(&mut out, BytePos::from_index(i));
                    } else {
                        out.saw(self.open, Wrote::Output, self.shells_d);
                        self.esc_at = self.body_esc_at;
                        self.at = At::Esc;
                    }
                }
            }
            out.saw(self.open, Wrote::Marks, self.shells_d);
        }
        self.cut = match self.d {
            DAt::At(d) => Some((
                d,
                out.after
                    .unwrap_or_default()
                    .ran(self.known, self.open, self.shells_d),
            )),
            DAt::Unseen | DAt::Earlier => None,
        };
        out.all.ran(self.known, self.open, self.shells_d)
    }

    /// Whether the last read carried the shell's `D`, with its token.
    pub fn shells_d(&self) -> ShellsD {
        self.shells_d
    }

    /// **Where the last read is to be cut**, if anywhere: where its first
    /// `D` of the shell's, with its token, began, when it began in that
    /// read, and what the bytes from there on say ([`Ran`]), read as a read
    /// of their own (`cut.rs`).
    pub fn cut(&self) -> Option<(BytePos, Ran)> {
        self.cut.map(|(d, after)| (d.0, after))
    }

    /// A body ended: acted on if it is an `OSC 133` command mark, and
    /// output of the command running if it is another control string.
    fn ended(&mut self, out: &mut Outs, end: BytePos) {
        let body = core::mem::take(&mut self.body);
        if !self.mark(&body, end) {
            out.saw(self.open, Wrote::Output, self.shells_d);
        }
        // From the shell's first `D` on, what the bytes say is read again,
        // as a read of their own would be.
        if out.after.is_none() && self.d != DAt::Unseen {
            let mut after = Out::default();
            after.saw(self.open, Wrote::Marks, self.shells_d);
            out.after = Some(after);
        }
        self.body = body;
    }

    /// **The marks the last read carried** ([`Timeline`]).
    pub fn marks(&self) -> Timeline {
        self.marks
    }

    /// The body of a mark ending just before `end`, acted on if it is an
    /// `OSC 133` command mark and kept in the read's [`Timeline`] if it
    /// has a letter, as the core keeps one (`baud_core::MarkEvent`):
    /// whether it is one.
    fn mark(&mut self, body: &[u8], end: BytePos) -> bool {
        let Some(rest) = body.strip_prefix(b"133;") else {
            return false;
        };
        let (letter, fields) = match rest.iter().position(|&b| b == b';') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        // **A body at the bound was cut there**, the core's `OSC_MAX`: its
        // last field is not the one sent, so a token in it is not read, and
        // a command line in it names nothing past its whole words.
        let tail = match body.len() >= BODY {
            true => Tail::Cut,
            false => Tail::Whole,
        };
        let whole = match tail {
            Tail::Cut => &fields[..fields.iter().rposition(|&b| b == b';').unwrap_or(0)],
            Tail::Whole => fields,
        };
        let token = options(whole)
            .find(|(name, v)| *name == b"baud_token" && !v.is_empty())
            .map(|(_, v)| Token::of(v));
        let shells = match letter {
            b"C" => {
                self.started(token, Named::of(fields, tail));
                ShellsD::No
            }
            b"D" => self.done(token),
            _ => ShellsD::No,
        };
        if let Some(&letter) = rest.first() {
            self.marks.push(Mark {
                at: BytePos::from_index(end.index().saturating_sub(1)),
                letter: StreamByte::from_stream(letter),
                shells,
                left: Left::of(self.open),
            });
        }
        true
    }

    /// A `C`: a command not local opens; one with the shell's own token
    /// naming a program here, or nothing, opens too, and ends what is open
    /// when it names a program here, its shell having started another. One
    /// that names nothing ends nothing. One that `exec`s a program here in
    /// the shell's place ends the shell's marks: its token is forgotten,
    /// and the next shell, if it marks its prompts, brings its own.
    fn started(&mut self, token: Option<Token>, named: Named) {
        // Only the token baud made for the shell is the shell's; a `C` with
        // another, or with one where baud made none, is a program's, and
        // carries none.
        let token = match (self.known, token) {
            (Some(k), Some(t)) if k == t => Some(t),
            _ => None,
        };
        let open = self.open;
        match (named, open) {
            (Named::Elsewhere, Open::Remote(Some(t))) if token != Some(t) => {}
            (Named::Elsewhere, _) => self.open = Open::Remote(token),
            // A program's own `C` ends nothing of the shell's.
            (Named::Here | Named::Replaced, Open::Remote(_)) if !self.ends(token) => {}
            (Named::Here | Named::Replaced, Open::Here(_) | Open::Unnamed(_))
                if token.is_none() => {}
            (Named::Replaced, _) if token.is_none() && self.known.is_some() => {}
            (Named::Replaced, _) => {
                self.open = Open::No;
                self.known = None;
            }
            (Named::Here, _) => self.open = token.map_or(Open::No, Open::Here),
            (Named::Nothing, Open::No | Open::Here(_) | Open::Unnamed(_)) => {
                if let Some(t) = token {
                    self.open = Open::Unnamed(t);
                }
            }
            (Named::Nothing, Open::Remote(_)) => {}
        }
    }

    /// A `D`: what is open ends if the mark carries its shell's token, or
    /// with no token ever seen. Whether it was the shell's own.
    fn done(&mut self, token: Option<Token>) -> ShellsD {
        if self.open != Open::No && self.ends(token) {
            self.open = Open::No;
        }
        if self.known.is_none() || token != self.known {
            return ShellsD::No;
        }
        self.shells_d = ShellsD::Yes;
        if self.d == DAt::Unseen {
            self.d = match self.esc_at {
                Some(at) => DAt::At(at),
                None => DAt::Earlier,
            };
        }
        ShellsD::Yes
    }

    /// Whether a mark carrying `token` may end what is open.
    fn ends(&self, token: Option<Token>) -> bool {
        match self.open {
            Open::No => false,
            Open::Remote(Some(t)) | Open::Here(t) | Open::Unnamed(t) => token == Some(t),
            Open::Remote(None) => match self.known {
                None => true,
                Some(k) => token == Some(k),
            },
        }
    }
}

/// Whether bytes in a read were the shell's marks or a program's output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wrote {
    Marks,
    Output,
}

/// **What a read was written under, as its scan went**: the weightiest
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
    /// `open` was open while the read wrote `what`, `shells_d` whether the
    /// read had carried the shell's own `D` by then: a command named not
    /// local counts whatever was written under it, the others only output,
    /// and output under none is the shell's only after that `D`.
    fn saw(&mut self, open: Open, what: Wrote, shells_d: ShellsD) {
        let under = match (open, what, shells_d) {
            (Open::Remote(_), _, _) => Out::Remote,
            (Open::Here(_), Wrote::Output, _) => Out::Here,
            (Open::Unnamed(_), Wrote::Output, _) => Out::Unnamed,
            (Open::No, Wrote::Output, ShellsD::No) => Out::Unmarked,
            (Open::No, Wrote::Output, ShellsD::Yes)
            | (Open::No | Open::Here(_) | Open::Unnamed(_), Wrote::Marks, _) => Out::Nothing,
        };
        *self = (*self).max(under);
    }

    /// The read's verdict, the shell's token `known` or not, `open` what is
    /// open as it ends, `shells_d` whether it carried the shell's own `D`:
    /// a read that wrote nothing but marks, or nothing but after that `D`,
    /// is the shell's only if it carried the `D` and no command is open
    /// after it, and the open command's while one is, so a `D` without the
    /// token, a body left open at the read's end, or a read with no `D` at
    /// all, is not the shell's.
    fn ran(self, known: Option<Token>, open: Open, shells_d: ShellsD) -> Ran {
        match (self, known) {
            (Out::Remote, _) => Ran::Elsewhere,
            (Out::Unnamed, _) => Ran::Command,
            (Out::Unmarked, _) => Ran::Unmarked,
            (Out::Here, _) => Ran::Here,
            (Out::Nothing, Some(_)) => match open {
                Open::No if shells_d == ShellsD::Yes => Ran::Shell,
                Open::No => Ran::Unmarked,
                Open::Here(_) => Ran::Here,
                Open::Unnamed(_) => Ran::Command,
                Open::Remote(_) => Ran::Elsewhere,
            },
            (Out::Nothing, None) => Ran::Unmarked,
        }
    }
}

/// The `name=value` options of a mark's fields; a field with no `=` names
/// nothing.
fn options(fields: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    fields.split(|&b| b == b';').filter_map(|f| {
        let eq = f.iter().position(|&b| b == b'=')?;
        Some((&f[..eq], &f[eq + 1..]))
    })
}

/// The command line of the first of a `C` mark's fields that carries one
/// ([`field_line`]), as the shell tests read it back.
#[cfg(test)]
pub(crate) fn command_line(fields: &[u8]) -> Option<Vec<u8>> {
    fields.split(|&b| b == b';').find_map(field_line)
}

/// **The command line one of a `C` mark's fields carries**, decoded:
/// kitty's `cmdline=`, "encoded by %q", one shell word read back, or its
/// `cmdline_url=`, "UTF-8 URL %-escaped text". None for any other field.
fn field_line(f: &[u8]) -> Option<Vec<u8>> {
    let eq = f.iter().position(|&b| b == b'=')?;
    let v = &f[eq + 1..];
    match &f[..eq] {
        b"cmdline_url" => Some(url_decoded(v)),
        b"cmdline" => Some(first_word(v).unwrap_or_default()),
        _ => None,
    }
}

/// `v` with each `%XX` decoded: kitty's "cmdline as UTF-8 URL %-escaped
/// text". A `%` not followed by two hex digits stays as it is.
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

/// The commands that run the word after them, which is the program:
/// `command`, `exec`, `env`, `nice`, `time` and `sudo`.
const PREFIXES: [&[u8]; 6] = [b"command", b"exec", b"env", b"nice", b"time", b"sudo"];

/// **What the command line `line` names**: its program, the first word past
/// `NAME=value` assignments, the [`PREFIXES`] and the options after one,
/// unquoted as the shell would, its last path component named by the paths
/// worker's list (`probe::front_of`); one `exec` puts in the shell's place
/// not local, or replaced; nothing if it has none, or if the word it would
/// be runs to a cut end (`tail`), a quoting left open there included.
fn names(line: &[u8], tail: Tail) -> Named {
    let mut rest = line;
    let mut after_prefix = false;
    let mut replaced = false;
    while let Some((word, next)) = word_at(rest) {
        if tail == Tail::Cut && next.is_empty() {
            return Named::Nothing;
        }
        rest = next;
        if is_assignment(&word) {
            continue;
        }
        if PREFIXES.contains(&word.as_slice()) {
            after_prefix = true;
            replaced |= word == b"exec";
            continue;
        }
        if after_prefix && word.first() == Some(&b'-') {
            continue;
        }
        let name = word.rsplit(|&b| b == b'/').next().unwrap_or(&word);
        return match std::str::from_utf8(name) {
            Ok("") => Named::Nothing,
            Ok(name)
                if crate::probe::front_of(&[name.to_string()])
                    == crate::paths::Front::Elsewhere =>
            {
                Named::Elsewhere
            }
            _ if replaced => Named::Replaced,
            _ => Named::Here,
        };
    }
    match replaced {
        true => Named::Replaced,
        false => Named::Nothing,
    }
}

/// `NAME=value`, a shell variable's assignment before the command.
fn is_assignment(word: &[u8]) -> bool {
    let Some(eq) = word.iter().position(|&b| b == b'=') else {
        return false;
    };
    let name = &word[..eq];
    name.first()
        .is_some_and(|&b| b == b'_' || b.is_ascii_alphabetic())
        && name.iter().all(|&b| b == b'_' || b.is_ascii_alphanumeric())
}

/// The first shell word of `s`, unquoted: what `printf %q` of a whole
/// command line gives back as the line.
fn first_word(s: &[u8]) -> Option<Vec<u8>> {
    word_at(s).map(|(w, _)| w)
}

/// **The next shell word of `s` and what follows it**, unquoted: blanks
/// before it skipped; `'...'` taken as it is, `"..."` with `\` before `"`,
/// `\`, `$` and a backquote, `$'...'` with its C escapes, and `\` outside
/// quotes taking the next byte. A blank, a newline, or one of `;&|()<>`
/// outside quotes ends it. None when nothing but blanks is left.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The scan for a window whose shell baud handed the token `t`.
    fn shell(t: &str) -> Commands {
        Commands::new(Some(&ShellToken::given(t)))
    }

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
        let mut one = shell("T");
        let mut all = c("T", "ssh localhost cat x");
        all.extend_from_slice(LINK);
        all.extend_from_slice(&d("T"));
        assert_eq!(one.read(&all), Ran::Elsewhere, "one read");
        assert_eq!(
            one.read(b"$ "),
            Ran::Unmarked,
            "the prompt after it: not ssh's"
        );
        let mut each = shell("T");
        assert_eq!(each.read(&c("T", "ssh localhost cat x")), Ran::Elsewhere);
        assert_eq!(each.read(LINK), Ran::Elsewhere, "the link");
        assert_eq!(each.read(&d("T")), Ran::Elsewhere, "the read that ends it");
        assert_eq!(each.read(b"$ "), Ran::Unmarked);
        let mut ls = shell("T");
        let mut local = c("T", "ls --hyperlink");
        local.extend_from_slice(LINK);
        local.extend_from_slice(&d("T"));
        assert_eq!(ls.read(&local), Ran::Here, "the control: ls");
    }

    /// **A mark split across two reads at every cut is read whole**.
    /// Named break: start a fresh scan at each read -- a split mark is
    /// lost.
    #[test]
    fn a_mark_split_at_every_cut_is_read_whole() {
        let m = c("T", "ssh h");
        for cut in 1..m.len() {
            let mut s = shell("T");
            let (a, b) = m.split_at(cut);
            let _ = s.read(a);
            let _ = s.read(b);
            assert_eq!(s.read(LINK), Ran::Elsewhere, "split after {cut}");
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
        let mut s = shell("T");
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
        let mut fish = Commands::default();
        let _ = fish.read(b"\x1b]133;C;cmdline_url=ssh%20h\x07");
        assert_eq!(fish.read(LINK), Ran::Elsewhere, "fish's own C");
        let _ = fish.read(b"\x1b]133;D;0\x07");
        assert_eq!(fish.read(LINK), Ran::Unmarked, "no token ever: a plain D");
    }

    /// **The shell's next command ends one whose `D` never came**: a `C`
    /// for `ls` with the same token. A `C` naming another remote program
    /// keeps it open. Named break: end only at a `D` -- the window stays
    /// remote.
    #[test]
    fn the_shells_next_command_ends_one_whose_end_never_came() {
        let mut s = shell("T");
        let _ = s.read(&c("T", "ssh h"));
        let _ = s.read(&c("T", "mosh h"));
        assert_eq!(s.read(LINK), Ran::Elsewhere, "another remote one");
        let _ = s.read(&c("T", "ls"));
        assert_eq!(s.read(LINK), Ran::Here, "the next command");
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
            let mut open = shell("T");
            let _ = open.read(&c("T", "ssh h"));
            let _ = open.read(bare);
            assert_eq!(open.read(LINK), Ran::Elsewhere, "after an ssh: {bare:?}");
            let mut none = shell("T");
            let _ = none.read(&c("T", "ls"));
            let _ = none.read(bare);
            assert_eq!(none.read(LINK), Ran::Command, "none open: {bare:?}");
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
        assert_eq!(Commands::default().read(b"% "), Ran::Unmarked, "no token");
        let mut s = shell("T");
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
        let mut fish4 = shell("T");
        let _ = fish4.read(b"\x1b]133;A;aid=1\x07> ");
        assert_eq!(fish4.read(b"ssh h\r\n"), Ran::Unmarked, "the echo");
        assert_eq!(fish4.read(LINK), Ran::Unmarked, "ssh's output");
        let mut s = shell("T");
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
    }

    /// **`exec` puts another program in the shell's place, and its token
    /// goes with it**: after the shell's `C` for `exec zsh` the token is
    /// forgotten, so every read after is unmarked, and the next shell's own
    /// token, which baud never handed it, is a program's and lowers
    /// nothing. Named breaks: take `exec zsh` as a command named here -- the
    /// reads after it stay its, and a gone writer there would read local;
    /// adopt a token from the stream -- the next shell's `C` opens a command
    /// named here.
    #[test]
    fn exec_takes_the_shells_token_with_it() {
        let mut s = shell("T");
        let _ = s.read(&c("T", "exec zsh"));
        assert_eq!(s.read(b"x\r\n"), Ran::Unmarked, "the shell is gone");
        let _ = s.read(&c("U", "cat y"));
        assert_eq!(s.read(b"y\r\n"), Ran::Unmarked, "a token baud never made");
        let _ = s.read(&d("U"));
        assert_eq!(s.read(b"% "), Ran::Unmarked);
    }

    /// **B2: no token is taken from the stream**: with none handed to the
    /// shell, a remote program's tokened `C` for `ssh`, its output and its
    /// tokened `D` leave every read after them unmarked, never the
    /// shell's, so nothing they say lowers a row. Named break: adopt the
    /// first token seen -- the read after the `D` is the shell's.
    #[test]
    fn no_token_is_taken_from_the_stream() {
        let mut s = Commands::default();
        let _ = s.read(&c("x", "ssh h"));
        let _ = s.read(LINK);
        let _ = s.read(&d("x"));
        assert_eq!(s.read(b"/etc/passwd\r\n"), Ran::Unmarked, "after the D");
        let _ = s.read(&c("x", "ls"));
        assert_eq!(s.read(b"y\r\n"), Ran::Unmarked, "under a C naming ls");
    }

    /// **B3: a read is the shell's only once no command is open**: under
    /// `cat x`, opened with the shell's token, a read holding only a `D`
    /// without the token, only an `A`, or ending inside a control string,
    /// is `cat`'s; the shell's own `D` then makes the next read the
    /// shell's. Named break: take a read of marks alone as the shell's --
    /// the untokened `D` reads as the shell's.
    #[test]
    fn a_read_is_the_shells_only_once_no_command_is_open() {
        let mut s = shell("T");
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
    }

    /// **B2's guards, each on its own**: with nothing open, a read of marks
    /// alone -- fish 4's `A`, or a `D` with another token -- is unmarked;
    /// output before the shell's `D` keeps the whole read unmarked; a `D`
    /// is the shell's only with its token, and only then is the link after
    /// it the shell's. Named breaks, one an assert: the shell's from marks
    /// alone under no command (`Out::ran`'s `Open::No` arm without its
    /// `shells_d` test) -- the `A` alone and the foreign `D` read as the
    /// shell's; output under no command counted as marks (`Out::saw`'s
    /// `ShellsD::No` arm as `Out::Nothing`) -- the job's line and the
    /// prompt read as the shell's; any token taken as the shell's in
    /// `Commands::done` -- the foreign `D` and its link read as the
    /// shell's.
    #[test]
    fn b2s_guards_each_hold_alone() {
        let mut s = shell("T");
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
    }

    /// **A `C` after the shell's `D` in one read is its command's**: one
    /// naming `cat` is here, one naming nothing the samples'. Named break:
    /// the shell's whenever the read carried its `D`, whatever is open
    /// after it -- both read as the shell's.
    #[test]
    fn a_command_after_the_shells_d_is_its_own() {
        let mut s = shell("T");
        let mut named = d("T");
        named.extend_from_slice(&c("T", "cat x"));
        assert_eq!(s.read(&named), Ran::Here, "a C naming cat");
        let mut unnamed = d("T");
        unnamed.extend_from_slice(b"\x1b]133;C;baud_token=T\x07");
        assert_eq!(s.read(&unnamed), Ran::Command, "a C naming nothing");
    }

    /// **A read is cut only at the shell's own `D`, begun in it**: under
    /// `ssh h`, ssh's last line, the shell's `D` and its prompt are cut at
    /// the `D`, the bytes from it the shell's; a `D` without the token, or
    /// with another, cuts nothing, and neither does the shell's `D` begun
    /// in the read before; of two of the shell's `D`s the first cuts.
    /// Named break: the cut taken at any `D` -- the forged ones cut.
    #[test]
    fn a_read_is_cut_only_at_the_shells_own_d() {
        let at = |s: &Commands| s.cut().map(|(d, after)| (d.index(), after));
        let mut s = shell("T");
        let _ = s.read(&c("T", "ssh h"));
        let end = [b"bye\r\n".as_slice(), &d("T"), b"% "].concat();
        assert_eq!(s.read(&end), Ran::Elsewhere, "the read uncut");
        assert_eq!(at(&s), Some((5, Ran::Shell)), "cut at the shell's D");
        let _ = s.read(&c("T", "ssh h"));
        let untokened = b"x\x1b]133;D;0\x07% ".to_vec();
        let foreign = [b"x".as_slice(), &d("X"), b"% "].concat();
        for forged in [untokened, foreign] {
            assert_eq!(s.read(&forged), Ran::Elsewhere);
            assert_eq!(at(&s), None, "{:?}", forged.escape_ascii().to_string());
        }
        let whole = d("T");
        let _ = s.read(&whole[..3]);
        let _ = s.read(&[&whole[3..], b"% ".as_slice()].concat());
        assert_eq!(at(&s), None, "split between two reads");
        let _ = s.read(&c("T", "ssh h"));
        let two = [b"y".as_slice(), &d("T"), b"% ", &d("T"), b"% "].concat();
        let _ = s.read(&two);
        assert_eq!(at(&s), Some((1, Ran::Shell)), "the first of two");
    }

    /// **With no token, a command naming a program here ends a remote
    /// one**, as fish 4's own marks need, which carry none. Named break:
    /// keep what is open on a `C` with no token.
    #[test]
    fn with_no_token_a_local_command_ends_a_remote_one() {
        let mut s = Commands::default();
        let _ = s.read(b"\x1b]133;C;cmdline_url=ssh%20h\x07");
        assert_eq!(s.read(LINK), Ran::Elsewhere, "ssh's");
        let _ = s.read(b"\x1b]133;C;cmdline_url=ls\x07");
        assert_eq!(s.read(LINK), Ran::Unmarked, "ended by the next");
    }

    /// **The program is the first word past assignments, the prefixes and
    /// their options, unquoted as the shell would**, its last path
    /// component named: every shape of `ssh` names one not local, and the
    /// same shapes of `ls` do not. Encoded as kitty does in both forms:
    /// `cmdline=` with `printf %q`'s quoting, `cmdline_url=` with `%XX`.
    /// Named break: take the line's first word as it is -- the prefixed and
    /// quoted ones read as `sudo`, `'ssh'` and the like.
    #[test]
    fn the_program_is_the_first_word_past_prefixes_and_quotes() {
        let shapes = [
            "{p} host",
            "  {p}\thost",
            "/usr/bin/{p} -p 22 host",
            "'{p}' host",
            "\"{p}\" host",
            "\\{p} host",
            "X=1 Y='a b' {p} host",
            "command {p} host",
            "exec {p} host",
            "env -i {p} host",
            "nice -10 {p} host",
            "time {p} host",
            "sudo -E {p} host",
            "{p}; ls",
            "{p}|cat",
        ];
        for shape in shapes {
            for (p, want) in [("ssh", true), ("mosh", true), ("ls", false)] {
                let line = shape.replace("{p}", p);
                assert_eq!(
                    names(line.as_bytes(), Tail::Whole) == Named::Elsewhere,
                    want,
                    "{line:?}"
                );
                // kitty's two encodings of the line, read back.
                let url: String = line
                    .bytes()
                    .map(|b| match b {
                        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'/' | b'-' | b'.' => {
                            char::from(b).to_string()
                        }
                        _ => format!("%{b:02X}"),
                    })
                    .collect();
                assert_eq!(
                    names(&url_decoded(url.as_bytes()), Tail::Whole) == Named::Elsewhere,
                    want,
                    "{url}"
                );
                let mut q = String::new();
                for ch in line.chars() {
                    match ch {
                        '\t' => q.push_str("$'\\t'"),
                        c if c.is_ascii_alphanumeric() || "/-.=_".contains(c) => q.push(c),
                        c => {
                            q.push('\\');
                            q.push(c);
                        }
                    }
                }
                let back = first_word(q.as_bytes()).unwrap_or_default();
                assert_eq!(back, line.as_bytes(), "%q of {line:?}: {q}");
            }
        }
        assert_eq!(names(b"", Tail::Whole), Named::Nothing, "nothing");
        assert_eq!(
            names(b"FOO=ssh", Tail::Whole),
            Named::Nothing,
            "an assignment alone"
        );
        assert!(
            names("caf\u{e9}=1 ssh h".as_bytes(), Tail::Whole) == Named::Here,
            "not a variable's name: the program itself"
        );
        assert_eq!(
            first_word(b"$'ssh\\x20h\\101'"),
            Some(b"ssh hA".to_vec()),
            "$'...'"
        );
    }

    /// **A command line longer than the core keeps of a control string
    /// still opens its command in the core's prompt log, ends it with its
    /// status, and opens the scan**: the core clips the `C` at `OSC_MAX` and
    /// still reads it as a `C` (`Term::prompts`), as it does a short one,
    /// the control; the token and the first word come first, so the scan
    /// reads them from the part both keep. Named break: drop a body longer
    /// than [`BODY`] in the scan -- the long command opens nothing.
    #[test]
    fn a_long_command_line_still_reaches_the_cores_marks() {
        use baud_domain::{BytePos, ExitCode, ExitStatus, ReadLen, WinSize};
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        for words in [1, 2000] {
            let mut owned = baud_core::Owned::new(&ws);
            let mut t = baud_core::Term::new(owned.storage(), &ws);
            let line = format!("ssh{}", "\\ x".repeat(words));
            let c = format!("\x1b]133;C;baud_token=t;cmdline={line}\x07");
            let bytes = format!(
                "\x1b]133;A\x07$ \x1b]133;B\x07\r\n{c}out\r\n\x1b]133;D;3;baud_token=t\x07\x1b]133;A\x07$ "
            );
            t.write_at(BytePos::ZERO, bytes.as_bytes());
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let statuses: Vec<ExitStatus> = t.prompts().map(|p| p.status).collect();
            assert!(
                statuses.contains(&ExitStatus::Exited(ExitCode::of(3))),
                "{} bytes of C: {statuses:?}",
                c.len()
            );
            let mut scan = shell("t");
            assert_eq!(scan.read(c.as_bytes()), Ran::Elsewhere, "{} bytes", c.len());
            assert_eq!(scan.read(b"\x1b]133;D;3;baud_token=t\x07"), Ran::Elsewhere);
            assert_eq!(scan.read(b"$ "), Ran::Unmarked, "closed by the shell's D");
        }
    }

    /// **A program word cut at [`BODY`] names nothing**: a `C` whose program
    /// path runs past the bound, `/usr/a...a/ssh`, ends no remote command,
    /// where its cut word, `a...a`, was read as a program here and ended
    /// the `ssh` the same shell had opened. The control: a short local
    /// path ends it. Named break: name the cut word.
    #[test]
    fn a_cut_program_word_names_nothing() {
        for (path, closed) in [
            (String::from("/usr/a/ls"), true),
            (format!("/usr/{}/ssh", "a".repeat(BODY)), false),
        ] {
            let mut scan = shell("t");
            let open = scan.read(b"\x1b]133;C;baud_token=t;cmdline=ssh\x07");
            assert_eq!(open, Ran::Elsewhere, "opened");
            let c = format!("\x1b]133;C;baud_token=t;cmdline={path}\x07");
            scan.read(c.as_bytes());
            let after = match closed {
                true => Ran::Here,
                false => Ran::Elsewhere,
            };
            assert_eq!(scan.read(b"$ "), after, "{} bytes of C", c.len());
        }
    }

    /// A timeline of marks `(terminator, letter, left)`, as a read that
    /// began with nothing open ended them.
    fn timeline(each: &[(usize, u8, Left)]) -> Timeline {
        let mut t = Timeline::begun(Left::Closed);
        for &(at, letter, left) in each {
            t.push(Mark {
                at: BytePos::from_index(at),
                letter: StreamByte::from_stream(letter),
                shells: ShellsD::No,
                left,
            });
        }
        t
    }

    /// **The scan keeps each mark at its terminator, as the core does**:
    /// `ESC ] 133 ; A BEL` ends at its `BEL`, and `ESC ] 133 ; D ESC \` at
    /// its `\`; a mark with no letter is kept by neither; and the shell's
    /// `D` with its token is told from a `D` without one. Named break:
    /// keep a mark at the offset past its terminator -- every offset is one
    /// more.
    #[test]
    fn the_scan_keeps_each_mark_where_the_core_does() {
        let mut scan = shell("t");
        let bytes = b"\x1b]133;A\x07$ \x1b]133;\x07\x1b]133;D;0;baud_token=t\x1b\\\x1b]133;D\x07";
        let _ = scan.read(bytes);
        let got: Vec<(usize, u8, ShellsD)> = scan
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
        // The same offsets the core records.
        let ws = baud_domain::WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let mut owned = baud_core::Owned::new(&ws);
        let mut t = baud_core::Term::new(owned.storage(), &ws);
        let _ = t.take_record();
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, baud_domain::ReadLen::from_index(bytes.len()));
        let core: Vec<(usize, u8)> = t
            .take_record()
            .marks()
            .iter()
            .map(|m| (m.at.index(), m.letter))
            .collect();
        let ours: Vec<(usize, u8)> = got.iter().map(|&(a, l, _)| (a, l)).collect();
        assert_eq!(core, ours, "the core's marks");
    }

    /// **The shell's `D` with its token closes whatever the scan believed
    /// open**: from every state the scan can hold under a window token --
    /// nothing open, a remote command with no token or with the window's,
    /// one named here, one naming nothing -- and for any token, the `D`
    /// carrying it leaves nothing open and is the shell's. What a re-sync
    /// rests on (`vouch::Resync`): after it the scan's state depends on
    /// nothing before. Named break: a remote command opened with no token
    /// ends only at a `D` naming none (`Commands::ends`) -- it stays open.
    #[test]
    fn the_shells_d_closes_every_state() {
        for token in ["t", "0123456789abcdef", "x;y", ""] {
            let k = Token::of(token.as_bytes());
            for open in [
                Open::No,
                Open::Remote(None),
                Open::Remote(Some(k)),
                Open::Here(k),
                Open::Unnamed(k),
            ] {
                let mut scan = shell(token);
                scan.open = open;
                assert_eq!(
                    scan.done(Some(k)),
                    ShellsD::Yes,
                    "{open:?}, token {token:?}"
                );
                assert_eq!(scan.open, Open::No, "{open:?}, token {token:?}");
            }
        }
    }

    /// **A part's marks match the core's only offset for offset and letter
    /// for letter** (`vouch::Matched`): the scan's marks of a read match
    /// the core's record of it; moved a byte either way, or with another
    /// letter, they do not; and past the kept marks they do not. Named
    /// breaks: compare letters alone -- the moved marks match; compare
    /// offsets alone -- the other letter matches.
    #[test]
    fn marks_match_only_where_and_as_the_core_ran_them() {
        use crate::vouch::{Matched, Vouch, Why};
        let ws = baud_domain::WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let bytes = b"\x1b]133;A\x07$ \x1b]133;D;0;baud_token=t\x1b\\";
        let mut owned = baud_core::Owned::new(&ws);
        let mut t = baud_core::Term::new(owned.storage(), &ws);
        let _ = t.take_record();
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, baud_domain::ReadLen::from_index(bytes.len()));
        let rec = t.take_record();
        let mut scan = shell("t");
        let _ = scan.read(bytes);
        let ran = scan.marks();
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

    /// **A part keeps the marks whose terminators it fed, and begins with
    /// what the last before it left open**. Named break: keep a mark in
    /// the part that fed its first byte rather than its terminator
    /// (`from < at` and `at <= to`) -- the mark at 10 moves parts.
    #[test]
    fn a_part_keeps_the_marks_it_fed() {
        let at = BytePos::from_index;
        let t = timeline(&[(10, b'C', Left::Remote), (20, b'D', Left::Closed)]);
        let letters = |t: Timeline| t.marks().map(|m| m.letter().raw_byte()).collect::<Vec<_>>();
        assert_eq!(letters(t.part(at(0), at(30))), [b'C', b'D'], "both");
        assert_eq!(
            letters(t.part(at(0), at(10))),
            Vec::<u8>::new(),
            "short of C's end"
        );
        assert_eq!(letters(t.part(at(10), at(20))), [b'C'], "C's end alone");
        assert_eq!(
            t.part(at(11), at(20)).open_at(at(11)),
            Left::Remote,
            "under C"
        );
        assert_eq!(
            t.part(at(21), at(30)).open_at(at(21)),
            Left::Closed,
            "after D"
        );
    }

    /// **Past the kept marks, every part reaching past the first dropped
    /// one is overflowed**, and the parts before it are not. Named break:
    /// a part overflowed only if it holds the dropped mark -- the part
    /// after it reads as matched with the marks before it unknown.
    #[test]
    fn a_part_past_the_kept_marks_is_overflowed() {
        let at = BytePos::from_index;
        let each: Vec<(usize, u8, Left)> = (1..=10).map(|k| (k * 10, b'A', Left::Closed)).collect();
        let t = timeline(&each);
        assert_eq!(t.marks().count(), MARKS);
        assert!(!t.part(at(0), at(85)).overflowed(), "before the ninth");
        assert!(t.part(at(0), at(95)).overflowed(), "the ninth");
        assert!(t.part(at(95), at(200)).overflowed(), "past it");
    }

    /// **The rest after a cut, cut again, is the rest after both**, and each
    /// part of a rest is that part of the whole, counted from the rest.
    /// Named break: rebase each mark one byte past the cut
    /// (`Timeline::rest`) -- the rest of a rest is a byte off.
    #[test]
    fn the_rest_of_a_rest_is_the_rest_of_the_sum() {
        let each: Vec<(usize, u8, Left)> = [3, 7, 9, 12, 20, 21, 22, 23, 24, 25]
            .iter()
            .enumerate()
            .map(|(k, &e)| {
                (
                    e,
                    b"ABCDEFGHIJ"[k],
                    [Left::Here, Left::Remote, Left::Closed][k % 3],
                )
            })
            .collect();
        let whole = timeline(&each);
        let at = BytePos::from_index;
        for a in 0..28 {
            for b in 0..(28 - a) {
                assert_eq!(
                    whole.rest(at(a)).rest(at(b)),
                    whole.rest(at(a + b)),
                    "{a} then {b}"
                );
                if b == 0 {
                    continue;
                }
                let shifted = whole.rest(at(a)).part(at(0), at(b));
                let direct = whole.part(at(a), at(a + b));
                assert_eq!(
                    shifted
                        .marks()
                        .map(|m| (m.at().index() + a, m.letter().raw_byte()))
                        .collect::<Vec<_>>(),
                    direct
                        .marks()
                        .map(|m| (m.at().index(), m.letter().raw_byte()))
                        .collect::<Vec<_>>(),
                    "{a} to {}",
                    a + b
                );
                assert_eq!(
                    shifted.overflowed(),
                    direct.overflowed(),
                    "{a} to {}",
                    a + b
                );
                assert_eq!(
                    shifted.open_at(at(0)),
                    direct.open_at(at(a)),
                    "{a} to {}",
                    a + b
                );
            }
        }
    }

    /// **What was open where a folder report came is what the scan read
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
        let mut scan = shell("t");
        let _ = scan.read(b"\x1b]133;C;baud_token=t;cmdline=ls\x07out\r\n");
        let prompt = b"\x1b]133;D;0;baud_token=t\x07\x1b]133;A\x07\x1b]7;file://h/tmp\x07% ";
        assert_eq!(scan.read(prompt), Ran::Shell, "the prompt's read");
        assert_eq!(scan.marks().open_at(at7(prompt, 0)), Left::Closed);
        let mut scan = shell("t");
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
    }
}
