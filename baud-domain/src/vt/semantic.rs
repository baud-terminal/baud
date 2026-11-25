//! `OSC 133`, the semantic prompt marks, read to their texts.
//!
//! **The governing text is Per Bothner's semantic-prompts proposal**
//! (`[[source: semantic-prompts]]`), the extension of FinalTerm's four
//! letters that iTerm2 documents (`[[source: iterm2-doc-escape-codes]]`) and
//! WezTerm cites as its definition (`[[source: wezterm-shell-integration]]`).
//! Options on `A` and `redraw=last`, which the proposal does not define,
//! are read here because shells send them: fish's built-in integration
//! sends `A;click_events=1` (`[[source: fish-terminal-compat]]`), and a
//! bash integration sends `A;redraw=last;cl=line;aid=<pid>`.
//!
//! "A terminal must ignore an option whose option-name it doesn't
//! recognize", so an unknown or malformed option is skipped and never
//! fails the mark.

/// The mark an `OSC 133` carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Semantic<'a> {
    /// `L`: "Do a fresh-line: If the cursor is the initial column ... do
    /// nothing. Otherwise, do the equivalent of `\r\n`."
    FreshLine,
    /// `A` or `N`: a fresh-line, then a prompt. With [`PromptKind::Initial`]
    /// (`k=i`, or no `k=`, which is what `A` has always meant) it starts a
    /// new command; with another kind it is kitty's spelling of a later
    /// prompt line -- kitty's zsh integration marks `PS2` with `A;k=s`, and
    /// nushell's reedline does too -- and starts none.
    Command(Opener, PromptOptions<'a>),
    /// `P`: "Explicit start of prompt. Optional after an A or N command."
    /// Its one applicable option is `k`. A real `zsh` sends `P` and no
    /// `A`, so an initial `P` where no prompt is open opens one.
    Prompt(PromptKind),
    /// `B` or `I`: the prompt has ended and what follows is what the user
    /// types, until `C` or another prompt (`B`), or until the end of the
    /// line (`I`).
    Input(InputEnd),
    /// `C`: "End of input, and start of output", with what its command
    /// line names and the token it carries ([`CommandStart`]).
    OutputStart(CommandStart),
    /// `D`: "End of current command", with what it says of the result.
    CommandDone(Done),
    /// A letter the proposal does not define, counted.
    Other,
}

/// Which of the two command marks opened a prompt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opener {
    /// `A`: "start a new command".
    FreshLineNewPrompt,
    /// `N`: "Same as OSC 133;A but may first implicitly terminate a previous
    /// command: If the options specify an aid and there is an active (open)
    /// command with matching aid, finish the innermost such command (as well
    /// as any other commands nested more deeply)."
    NewCommand,
}

/// Where a `B` or an `I` says the user's input ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputEnd {
    /// `B`: "terminated by a OSC 133;C or another prompt".
    AtMark,
    /// `I`: "terminated by end-of-line".
    AtEndOfLine,
}

/// Which line of a prompt a mark opens: the `k=` option.
///
/// A shell with a multi-line prompt marks *each* line -- kitty's own
/// zsh integration sends `OSC 133 ; A ; k=s` "just before starting to
/// draw the PS2 prompt", and fish marks its continuation lines the same
/// way -- so a terminal that took every mark as the start of a new block
/// would measure a block from its last line. The default when `k=` is
/// absent is the initial line.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PromptKind {
    /// `k=i`, or no `k=` at all: the first line of a prompt.
    #[default]
    Initial,
    /// `k=s`: a secondary prompt (`PS2`), drawn on a later line of the
    /// same block.
    Secondary,
    /// `k=r`: a right prompt, on the same line as the one before it.
    Right,
    /// `k=c`: a continuation line of the primary prompt.
    Continuation,
}

/// kitty's `redraw=`, with Ghostty's third value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Redraw {
    /// `redraw=1`, and the default: the shell redraws its prompt on a
    /// resize.
    Prompt,
    /// `redraw=0`: "the shell will not redraw the prompt on resize so it
    /// should not erase it".
    Never,
    /// `redraw=last`: "The shell supports redrawing only the LAST line of
    /// the prompt" -- bash.
    LastLine,
}

/// `cl=`: "a request from the application to the terminal to translate
/// clicks in the input area to cursor movement". "The terminal is free to
/// ignore this option, or to treat any specified value as `line`", and baud
/// treats every value as `line`, so `m`, `v`, `w` and a comma list all
/// read as it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClickToMove {
    Line,
}

/// kitty's `click_events=`: the shell takes a click as an SGR mouse report.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClickEvents {
    /// `1`: the report's row is the screen's.
    Absolute,
    /// `2`: the row is counted from the prompt's top line.
    Relative,
}

/// kitty's `special_key=`: which keys a click-to-move sends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpecialKey {
    /// `0`, and the default: the cursor keys.
    Arrows,
    /// `1`: `CSI 1 u` for right and `CSI 1 ; 1 u` for left, "Useful if
    /// arrow keys have side-effects like triggering auto complete".
    Special,
}

/// `move-keys=`, kept: up to [`MoveKeys::MAX`] bytes, split at its commas
/// as it is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MoveKeys {
    bytes: [u8; MoveKeys::MAX],
    len: u8,
}

impl MoveKeys {
    /// Four sequences and their three commas: room for an escape-free
    /// sequence of fifteen bytes each.
    pub const MAX: usize = 63;
    pub const NONE: MoveKeys = MoveKeys {
        bytes: [0; MoveKeys::MAX],
        len: 0,
    };

    /// The value as sent, or `None` when it is longer than this keeps or
    /// names no left and right: a cut sequence is a different key.
    pub fn of(v: &[u8]) -> Option<MoveKeys> {
        if v.len() > MoveKeys::MAX {
            return None;
        }
        let mut k = MoveKeys::NONE;
        k.bytes[..v.len()].copy_from_slice(v);
        k.len = v.len() as u8;
        let (l, r) = (k.nth(0)?, k.nth(1)?);
        (!l.is_empty() && !r.is_empty()).then_some(k)
    }

    fn nth(&self, n: usize) -> Option<&[u8]> {
        self.bytes[..usize::from(self.len)]
            .split(|&b| b == b',')
            .nth(n)
    }

    /// The sequence for a move left.
    pub fn left(&self) -> &[u8] {
        self.nth(0).unwrap_or(b"")
    }

    /// The sequence for a move right.
    pub fn right(&self) -> &[u8] {
        self.nth(1).unwrap_or(b"")
    }
}

/// An application identifier, `aid=`, kept as a hash of its bytes: a
/// command is matched to the one that opened it by equality and nothing
/// else, and the proposal's own suggestion is a process id. "If no `aid` is
/// specified, treat as an `aid` whose value is the empty string."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Aid(u32);

impl Aid {
    /// The empty string's.
    pub const EMPTY: Aid = Aid::of(b"");

    /// FNV-1a over the value's bytes.
    pub const fn of(s: &[u8]) -> Aid {
        let mut h: u32 = 0x811c_9dc5;
        let mut i = 0;
        while i < s.len() {
            h ^= s[i] as u32;
            h = h.wrapping_mul(0x0100_0193);
            i += 1;
        }
        Aid(h)
    }
}

/// The options an `A` or an `N` carries, each `None` when absent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PromptOptions<'a> {
    pub kind: PromptKind,
    pub redraw: Option<Redraw>,
    pub click: Option<ClickToMove>,
    pub click_events: Option<ClickEvents>,
    pub special_key: Option<SpecialKey>,
    /// `move-keys=left,right[,up,down]`: "key sequences to use for
    /// click-move mode", as sent. An option "can be any string of
    /// characters not including control characters ... or semicolon", so
    /// these are printable.
    pub move_keys: Option<&'a [u8]>,
    pub aid: Aid,
}

impl<'a> PromptOptions<'a> {
    /// Every option read from the `;`-separated fields after the letter.
    fn of(fields: &'a [u8]) -> PromptOptions<'a> {
        let mut o = PromptOptions {
            kind: PromptKind::Initial,
            redraw: None,
            click: None,
            click_events: None,
            special_key: None,
            move_keys: None,
            aid: Aid::EMPTY,
        };
        for (name, v) in named(fields) {
            match name {
                b"k" => o.kind = kind(v).unwrap_or(o.kind),
                b"redraw" => {
                    o.redraw = match v {
                        b"0" => Some(Redraw::Never),
                        b"1" => Some(Redraw::Prompt),
                        b"last" => Some(Redraw::LastLine),
                        _ => o.redraw,
                    }
                }
                // Any value is read as `line`, which the text permits; an
                // empty one names nothing.
                b"cl" if !v.is_empty() => o.click = Some(ClickToMove::Line),
                b"click_events" => {
                    o.click_events = match v {
                        b"1" => Some(ClickEvents::Absolute),
                        b"2" => Some(ClickEvents::Relative),
                        _ => o.click_events,
                    }
                }
                b"special_key" => {
                    o.special_key = match v {
                        b"0" => Some(SpecialKey::Arrows),
                        b"1" => Some(SpecialKey::Special),
                        _ => o.special_key,
                    }
                }
                b"move-keys" if !v.is_empty() => o.move_keys = Some(v),
                b"aid" => o.aid = Aid::of(v),
                _ => {}
            }
        }
        o
    }
}

/// A command's exit code, as `D` or `err=` gives it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExitCode(i32);

impl ExitCode {
    pub const fn of(code: i32) -> ExitCode {
        ExitCode(code)
    }
    pub const fn value(self) -> i32 {
        self.0
    }
}

/// What a `D` says of the command it ends, before the terminal knows
/// whether the command was ever run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Done {
    /// The positional exit code: "it must be an integer code, where 0 means
    /// the command succeeded, and other values indicate failure".
    pub code: Option<ExitCode>,
    /// `err=`, which "non-legacy terminals should give precedence to".
    pub err: Option<ErrValue>,
    pub aid: Aid,
}

/// `err=`: "an empty string is success, and any non-empty value (which need
/// not be an integer) is an error code"; `err=CANCEL` is the cancel the
/// text suggests.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrValue {
    Empty,
    Cancel,
    Code(ExitCode),
    Other,
}

/// How a command ended, as the terminal keeps it on the prompt that began
/// it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExitStatus {
    /// No `D` has ended it, or one that said nothing.
    Unreported,
    /// `D ; <code>` with no `err=`: `0` succeeded, any other failed.
    Exited(ExitCode),
    /// `err=` empty: success, whatever the code said.
    Succeeded,
    /// `err=` non-empty: "any non-empty value should be displayed as an
    /// error, even 0", with its code when it is an integer.
    Failed(Option<ExitCode>),
    /// Cancelled: `err=CANCEL`, or a `D` after the prompt with no `C`,
    /// which iTerm2 reads as "a command was aborted".
    Cancelled,
}

impl Done {
    /// The status, `ran` saying whether a `C` began the command's output.
    /// `err=` first, as the text says; then the abort, whose code iTerm2
    /// says "will be ignored"; then the positional code.
    pub fn status(self, ran: Ran) -> ExitStatus {
        match (self.err, ran, self.code) {
            (Some(ErrValue::Empty), _, _) => ExitStatus::Succeeded,
            (Some(ErrValue::Cancel), _, _) => ExitStatus::Cancelled,
            (Some(ErrValue::Code(c)), _, _) => ExitStatus::Failed(Some(c)),
            (Some(ErrValue::Other), _, _) => ExitStatus::Failed(None),
            (None, Ran::NoOutput, _) => ExitStatus::Cancelled,
            (None, Ran::Output, Some(c)) => ExitStatus::Exited(c),
            (None, Ran::Output, None) => ExitStatus::Unreported,
        }
    }
}

/// Whether a command reached its output: a `C` after its prompt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ran {
    NoOutput,
    Output,
}

impl<'a> Semantic<'a> {
    /// The mark named by the bytes after `133;`, whole.
    pub fn parse(rest: &'a [u8]) -> Semantic<'a> {
        let (letter, fields) = letter_of(rest);
        let runs = match letter {
            b"C" => CommandRuns::of(fields),
            _ => CommandRuns::Nothing,
        };
        Semantic::of(letter, fields, runs)
    }

    /// **The mark named by the bytes after `133;`, cut at the bound**: its
    /// options are the ones whose `;` arrived, and a `C`'s command line is
    /// read to the cut ([`CommandRuns::of_clipped`]).
    pub fn parse_clipped(rest: &'a [u8]) -> Semantic<'a> {
        let (letter, fields) = letter_of(rest);
        let whole = &fields[..fields.iter().rposition(|&b| b == b';').unwrap_or(0)];
        let runs = match letter {
            b"C" => CommandRuns::of_clipped(fields),
            _ => CommandRuns::Nothing,
        };
        Semantic::of(letter, whole, runs)
    }

    /// The mark of `letter` with the whole options `fields`, a `C` naming
    /// `runs`.
    fn of(letter: &[u8], fields: &'a [u8], runs: CommandRuns) -> Semantic<'a> {
        match letter {
            b"L" => Semantic::FreshLine,
            b"A" => Semantic::Command(Opener::FreshLineNewPrompt, PromptOptions::of(fields)),
            b"N" => Semantic::Command(Opener::NewCommand, PromptOptions::of(fields)),
            b"P" => Semantic::Prompt(PromptOptions::of(fields).kind),
            b"B" => Semantic::Input(InputEnd::AtMark),
            b"I" => Semantic::Input(InputEnd::AtEndOfLine),
            b"C" => Semantic::OutputStart(CommandStart {
                runs,
                token: ShellTokenHash::in_mark(fields),
            }),
            b"D" => Semantic::CommandDone(done(fields)),
            _ => Semantic::Other,
        }
    }
}

/// A mark's letter, the bytes before its first `;`, and its options.
fn letter_of(rest: &[u8]) -> (&[u8], &[u8]) {
    match rest.iter().position(|&b| b == b';') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, &rest[rest.len()..]),
    }
}

/// `D [; exit-code options]`: the first field is the code when it is an
/// integer, as Ghostty reads it, and an option otherwise.
fn done(fields: &[u8]) -> Done {
    let (first, rest) = match fields.iter().position(|&b| b == b';') {
        Some(i) => (&fields[..i], &fields[i + 1..]),
        None => (fields, &fields[fields.len()..]),
    };
    let code = integer(first);
    let options = if code.is_some() { rest } else { fields };
    let mut d = Done {
        code,
        err: None,
        aid: Aid::EMPTY,
    };
    for (name, v) in named(options) {
        match name {
            b"err" => {
                d.err = Some(match v {
                    b"" => ErrValue::Empty,
                    b"CANCEL" => ErrValue::Cancel,
                    _ => integer(v).map_or(ErrValue::Other, ErrValue::Code),
                })
            }
            b"aid" => d.aid = Aid::of(v),
            _ => {}
        }
    }
    d
}

/// The `name=value` fields of `s`; a field with no `=` names nothing.
fn named(s: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    s.split(|&b| b == b';').filter_map(|f| {
        let eq = f.iter().position(|&b| b == b'=')?;
        Some((&f[..eq], &f[eq + 1..]))
    })
}

fn kind(v: &[u8]) -> Option<PromptKind> {
    Some(match v {
        b"i" => PromptKind::Initial,
        b"s" => PromptKind::Secondary,
        b"r" => PromptKind::Right,
        b"c" => PromptKind::Continuation,
        _ => return None,
    })
}

/// A base-10 integer with an optional sign that fits an `i32`.
fn integer(s: &[u8]) -> Option<ExitCode> {
    let (neg, digits) = match s.split_first() {
        Some((b'-', d)) => (true, d),
        Some((b'+', d)) => (false, d),
        _ => (false, s),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut v: i64 = 0;
    for &b in digits {
        v = v.checked_mul(10)?.checked_add(i64::from(b - b'0'))?;
        if v > i64::from(i32::MAX) + 1 {
            return None;
        }
    }
    let v = if neg { -v } else { v };
    i32::try_from(v).ok().map(ExitCode)
}

/// **The window's shell token**: exactly sixteen ASCII hex digits, as baud
/// makes one for the shell it spawns and hands to its integration, which
/// sends it as the `baud_token=` option of its own `C` and `D` marks. No
/// other value is one: made only by [`ShellTokenHex::parse`], its field
/// private, and shown by `Debug` without its digits.
///
/// ```compile_fail,E0423
/// let _ = baud_domain::ShellTokenHex([baud_domain::StreamByte::from_stream(b'0'); 16]);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::ShellTokenHex;
/// assert!(ShellTokenHex::parse(b"0123456789abcdef").is_some());
/// assert!(ShellTokenHex::parse(b"0123456789abcde").is_none());
/// assert!(ShellTokenHex::parse(b"0123456789abcdefa").is_none());
/// assert!(ShellTokenHex::parse(b"0123456789abcdeg").is_none());
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ShellTokenHex([crate::StreamByte; 16]);

/// The digits are not shown.
impl core::fmt::Debug for ShellTokenHex {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ShellTokenHex(..)")
    }
}

impl ShellTokenHex {
    /// Its length: sixteen digits.
    pub const LEN: usize = 16;

    /// **The token `b` spells**, when it is exactly sixteen ASCII hex digits.
    pub fn parse(b: &[u8]) -> Option<ShellTokenHex> {
        let digits: [u8; ShellTokenHex::LEN] = b.try_into().ok()?;
        match digits.iter().all(u8::is_ascii_hexdigit) {
            true => Some(ShellTokenHex(digits.map(crate::StreamByte))),
            false => None,
        }
    }

    /// Whether `b` is this token's digits, byte for byte.
    pub(crate) fn is(&self, b: &[u8]) -> bool {
        b.len() == ShellTokenHex::LEN && self.0.iter().zip(b).all(|(t, &c)| t.raw_byte() == c)
    }
}

/// **The hash of the window's shell token** ([`ShellTokenHex`]), as the
/// core keeps it and compares a mark's against it: FNV-1a over the sixteen
/// digits. Made only from a token, never from a stream: a mark's value is
/// hashed only when it spells a token ([`ShellTokenHash::in_mark`]).
///
/// ```compile_fail,E0423
/// let _ = baud_domain::ShellTokenHash(0);
/// ```
///
/// ```compile_fail,E0308
/// let _ = baud_domain::ShellTokenHash::of(b"short");
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{ShellTokenHash, ShellTokenHex};
/// let hex = ShellTokenHex::parse(b"0123456789abcdef").expect("a token");
/// let mark = b"0;aid=1;baud_token=0123456789abcdef";
/// assert_eq!(ShellTokenHash::in_mark(mark), Some(ShellTokenHash::of(&hex)));
/// assert_eq!(ShellTokenHash::in_mark(b"0;baud_token=T"), None);
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ShellTokenHash(u64);

/// The hash is not shown.
impl core::fmt::Debug for ShellTokenHash {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ShellTokenHash(..)")
    }
}

impl ShellTokenHash {
    /// The hash of `token`'s digits.
    pub const fn of(token: &ShellTokenHex) -> ShellTokenHash {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < ShellTokenHex::LEN {
            h ^= token.0[i].raw_byte() as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        ShellTokenHash(h)
    }

    /// **The token a mark's options carry**: the first `baud_token=` option
    /// with a value, hashed when that value spells a token; none when there
    /// is no such option or its value is not sixteen hex digits. `options`
    /// is what follows the mark's letter, no more than its whole options.
    pub fn in_mark(options: &[u8]) -> Option<ShellTokenHash> {
        named(options)
            .find(|(name, v)| *name == b"baud_token" && !v.is_empty())
            .and_then(|(_, v)| ShellTokenHex::parse(v))
            .map(|t| ShellTokenHash::of(&t))
    }
}

/// **What a mark says of the window's shell token**, read against the token
/// the core holds: none held, so none is seen in any mark; the mark carries
/// it; or one is held and the mark does not carry it -- no `baud_token=`,
/// an empty one, or another value. Every value is valid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum TokenSeen {
    /// The core holds no shell token: none was handed to the window's
    /// shell, or the `exec` that replaced the shell took it.
    Absent,
    /// The mark carries the token the core holds.
    Shells,
    /// The core holds a token and the mark does not carry it.
    Other,
}

impl TokenSeen {
    /// What a mark carrying `seen` ([`ShellTokenHash::in_mark`]) says, the
    /// core holding `held`.
    pub fn of(held: Option<ShellTokenHash>, seen: Option<ShellTokenHash>) -> TokenSeen {
        match (held, seen) {
            (None, _) => TokenSeen::Absent,
            (Some(h), Some(s)) if h == s => TokenSeen::Shells,
            (Some(_), _) => TokenSeen::Other,
        }
    }
}

/// **Programs whose output is another machine's**: a path they print names
/// a file there, so nothing is looked up while one is in front, and a
/// command the shell names running one makes its output remote
/// ([`CommandRuns::of`]).
pub const ELSEWHERE: [&str; 24] = [
    // A shell on another machine.
    "ssh",
    "slogin",
    "autossh",
    "mosh",
    "mosh-client",
    "telnet",
    "rsh",
    "rlogin",
    // Eternal Terminal's client: a shell on another machine.
    "et",
    // Teleport's `tsh ssh`: a shell on another machine.
    "tsh",
    // kitty's `kitten ssh`; its other subcommands print no path of this
    // machine's that a shell would not.
    "kitten",
    // A container's own file system: `docker run`, `docker exec`.
    "docker",
    "podman",
    // A pod's file system, on another machine.
    "kubectl",
    // A Lima virtual machine's shell, whose root is not this machine's.
    "limactl",
    "lima",
    // An OrbStack machine's shell.
    "orb",
    "orbctl",
    // A Multipass virtual machine's shell.
    "multipass",
    // `vagrant ssh`: a virtual machine's shell.
    "vagrant",
    // `gcloud compute ssh`, `gcloud cloud-shell ssh`: another machine.
    "gcloud",
    // A container that shares the home folder and not the rest: its `/usr`
    // and `/etc` are the image's.
    "distrobox",
    "toolbox",
    // `virsh console`: a virtual machine's console.
    "virsh",
];

/// A program's name as [`ELSEWHERE`] spells it: a wrapper's leading `.`
/// and `-wrapped` taken off, and what follows a `:`.
pub fn spelled_program(name: &str) -> &str {
    let n = name.strip_prefix('.').unwrap_or(name);
    let n = n.strip_suffix("-wrapped").unwrap_or(n);
    n.split(':').next().unwrap_or(n)
}

/// **What the program a `C` mark's command line names runs as**
/// ([`CommandRuns::of`]): nothing named; a program on this machine; one
/// [`ELSEWHERE`] names; or a program on this machine that `exec` puts in
/// the shell's place, which ends the shell and its token. Every value is
/// valid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum CommandRuns {
    /// No command line, one with no program in it, or a program word cut
    /// at the bound.
    Nothing,
    Here,
    Elsewhere,
    Replaced,
}

/// Whether a command line, or a word of it, reached the bound it was cut
/// at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tail {
    Whole,
    Cut,
}

/// Whether a prefix came before the word being read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Prefixed {
    No,
    Yes,
}

/// The most a command line read back holds: the core keeps no more of a
/// control string.
const LINE_MOST: usize = 512;

/// **Bytes written into a fixed buffer**: what does not fit is dropped and
/// the buffer marked cut, so a word that overflowed names nothing.
struct Buf {
    bytes: [u8; LINE_MOST],
    len: usize,
    tail: Tail,
}

impl Buf {
    const fn new() -> Buf {
        Buf {
            bytes: [0; LINE_MOST],
            len: 0,
            tail: Tail::Whole,
        }
    }

    fn push(&mut self, b: u8) {
        match self.bytes.get_mut(self.len) {
            Some(slot) => {
                *slot = b;
                self.len += 1;
            }
            None => self.tail = Tail::Cut,
        }
    }

    fn extend(&mut self, s: &[u8]) {
        s.iter().for_each(|&b| self.push(b));
    }

    fn get(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// The commands that run the word after them, which is the program:
/// `command`, `exec`, `env`, `nice`, `time` and `sudo`.
const PREFIXES: [&[u8]; 6] = [b"command", b"exec", b"env", b"nice", b"time", b"sudo"];

impl CommandRuns {
    /// **What the options of a whole `C` mark name**: the command line of
    /// the first `cmdline` or `cmdline_url` option -- `cmdline=`, one shell
    /// word as `printf %q` gives it, read back, or `cmdline_url=`,
    /// `%XX`-escaped (`[[source: kitty-shell-integration]]`) -- and its
    /// program: the first word past `NAME=value` assignments, the prefixes
    /// `command`, `exec`, `env`, `nice`, `time` and `sudo` and the options
    /// after one, unquoted as the shell would, its last path component
    /// checked against [`ELSEWHERE`]. **[`ELSEWHERE`]'s 24 names are walked
    /// once a mark, at most**, and no byte outside the mark is read; no
    /// allocation, two buffers of 512 bytes on the stack.
    pub fn of(options: &[u8]) -> CommandRuns {
        command_of(options, Tail::Whole)
    }

    /// **As [`CommandRuns::of`], for the options of a mark cut at the
    /// bound**: the last option is not the one sent, so a program word that
    /// runs to its end names nothing.
    pub fn of_clipped(options: &[u8]) -> CommandRuns {
        command_of(options, Tail::Cut)
    }
}

/// The command line of the first option of `fields` that carries one, its
/// last option cut where `tail` says.
fn command_of(fields: &[u8], tail: Tail) -> CommandRuns {
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
        let Some(eq) = f.iter().position(|&b| b == b'=') else {
            continue;
        };
        let v = &f[eq + 1..];
        let mut line = Buf::new();
        match &f[..eq] {
            b"cmdline_url" => url_decoded(v, &mut line),
            b"cmdline" => {
                let _ = word_at(v, &mut line);
            }
            _ => continue,
        }
        let line_tail = match line.tail {
            Tail::Cut => Tail::Cut,
            Tail::Whole => line_tail,
        };
        return names(line.get(), line_tail);
    }
    CommandRuns::Nothing
}

/// `v` with each `%XX` decoded onto `out`; a `%` not followed by two hex
/// digits stays as it is.
fn url_decoded(v: &[u8], out: &mut Buf) {
    let hex = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
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
}

/// **What the command line `line` names**: a word that would be its
/// program running to a cut end names nothing.
fn names(line: &[u8], tail: Tail) -> CommandRuns {
    let mut rest = line;
    let mut prefixed = Prefixed::No;
    let mut replaced = CommandRuns::Nothing;
    loop {
        let mut word = Buf::new();
        let Some(next) = word_at(rest, &mut word) else {
            return replaced;
        };
        if (tail == Tail::Cut && next.is_empty()) || word.tail == Tail::Cut {
            return CommandRuns::Nothing;
        }
        rest = next;
        let w = word.get();
        if is_assignment(w) {
            continue;
        }
        if PREFIXES.contains(&w) {
            prefixed = Prefixed::Yes;
            if w == b"exec" {
                replaced = CommandRuns::Replaced;
            }
            continue;
        }
        if prefixed == Prefixed::Yes && w.first() == Some(&b'-') {
            continue;
        }
        let name = w.rsplit(|&b| b == b'/').next().unwrap_or(w);
        return match core::str::from_utf8(name) {
            Ok("") => CommandRuns::Nothing,
            Ok(n) if ELSEWHERE.contains(&spelled_program(n)) => CommandRuns::Elsewhere,
            _ => match replaced {
                CommandRuns::Replaced => CommandRuns::Replaced,
                _ => CommandRuns::Here,
            },
        };
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

/// **The next shell word of `s` onto `w`, unquoted, and what follows it**:
/// blanks before it skipped; `'...'` taken as it is, `"..."` with `\`
/// before `"`, `\`, `$` and a backquote, `$'...'` with its C escapes, and
/// `\` outside quoting taking the next byte. A blank, a newline, or one of
/// `;&|()<>` outside quoting ends it. None when nothing but blanks is left.
fn word_at<'s>(s: &'s [u8], w: &mut Buf) -> Option<&'s [u8]> {
    let start = s.iter().position(|&b| !matches!(b, b' ' | b'\t'))?;
    let s = &s[start..];
    if matches!(
        s.first(),
        Some(b'\n' | b';' | b'&' | b'|' | b'(' | b')' | b'<' | b'>')
    ) {
        return None;
    }
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
                w.extend(&s[i + 1..end]);
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
                let rest = ansi_c(&s[i + 2..], w);
                i = s.len() - rest.len();
            }
            c => {
                w.push(c);
                i += 1;
            }
        }
    }
    Some(s.get(i..).unwrap_or_default())
}

/// A `$'...'` string, `s` starting past its opening `'`, onto `w`, as
/// bash reads it: `\n`, `\t`, `\r`, `\e`, `\a`, `\\`, `\'`, `\"`, `\xHH` and
/// up to three octal digits. What follows its closing `'`.
fn ansi_c<'a>(s: &'a [u8], w: &mut Buf) -> &'a [u8] {
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
            w.extend(&[b'\\', c]);
        }
    }
    s.get(i + 1..).unwrap_or_default()
}

/// **What a `C` mark said**: what its command runs ([`CommandRuns`]) and
/// the token it carries, if one is spelled there
/// ([`ShellTokenHash::in_mark`]). Made only by [`Semantic::parse`] and
/// [`Semantic::parse_clipped`].
///
/// ```compile_fail,E0451
/// let _ = baud_domain::CommandStart { runs: baud_domain::CommandRuns::Nothing, token: None };
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{CommandRuns, Semantic};
/// let Semantic::OutputStart(c) = Semantic::parse(b"C;cmdline=ssh\\ h") else {
///     panic!("a C");
/// };
/// assert_eq!(c.runs(), CommandRuns::Elsewhere);
/// assert_eq!(c.token(), None);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CommandStart {
    runs: CommandRuns,
    token: Option<ShellTokenHash>,
}

impl CommandStart {
    pub fn runs(self) -> CommandRuns {
        self.runs
    }

    pub fn token(self) -> Option<ShellTokenHash> {
        self.token
    }
}

/// **What the shell's marks say is running**, stepped at each mark the
/// core recorded ([`MarkOpen::step`]): nothing; a command named
/// [`ELSEWHERE`]; one named on this machine; one naming nothing. The
/// payload is the token verdict of the mark that opened it: only a `C`
/// carrying the shell's own opens one here or one naming nothing, so
/// theirs is [`TokenSeen::Shells`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MarkOpen {
    #[default]
    No,
    Remote(TokenSeen),
    Here(TokenSeen),
    Unnamed(TokenSeen),
}

impl MarkOpen {
    /// **What is open after a mark of `letter`**, carrying `token` and, for
    /// a `C`, naming `runs`. A mark lowers nothing unless it carries the
    /// shell's token: a `C` naming a program elsewhere opens a remote
    /// command, and keeps one the shell opened unless it is the shell's
    /// own; the shell's `C` naming a program here, or nothing, opens it; one
    /// naming a program here ends what the shell opened, and one `exec`
    /// puts in the shell's place ends everything. A `D` ends what is open
    /// when it may ([`MarkOpen::ends`]). Any other letter changes nothing.
    ///
    /// ```compile_fail,E0308
    /// let _ = baud_domain::MarkOpen::No.step(b"\x1b]133;C\x07", baud_domain::TokenSeen::Absent, baud_domain::CommandRuns::Nothing);
    /// ```
    ///
    /// The legal path:
    ///
    /// ```
    /// use baud_domain::{CommandRuns, MarkOpen, StreamByte, TokenSeen};
    /// let c = StreamByte::from_stream(b'C');
    /// let open = MarkOpen::No.step(c, TokenSeen::Other, CommandRuns::Elsewhere);
    /// assert_eq!(open, MarkOpen::Remote(TokenSeen::Other));
    /// let d = StreamByte::from_stream(b'D');
    /// assert_eq!(open.step(d, TokenSeen::Other, CommandRuns::Nothing), open);
    /// assert_eq!(open.step(d, TokenSeen::Shells, CommandRuns::Nothing), MarkOpen::No);
    /// ```
    pub const fn step(
        self,
        letter: crate::StreamByte,
        token: TokenSeen,
        runs: CommandRuns,
    ) -> MarkOpen {
        match letter.raw_byte() {
            b'C' => self.started(token, runs),
            b'D' => match self.ends(token) {
                true => MarkOpen::No,
                false => self,
            },
            _ => self,
        }
    }

    /// A `C`.
    const fn started(self, token: TokenSeen, runs: CommandRuns) -> MarkOpen {
        let shells = matches!(token, TokenSeen::Shells);
        match (runs, self) {
            (CommandRuns::Elsewhere, MarkOpen::Remote(TokenSeen::Shells)) if !shells => self,
            (CommandRuns::Elsewhere, _) => MarkOpen::Remote(token),
            // A program's own `C` ends nothing of the shell's.
            (CommandRuns::Here | CommandRuns::Replaced, MarkOpen::Remote(_))
                if !self.ends(token) =>
            {
                self
            }
            (
                CommandRuns::Here | CommandRuns::Replaced,
                MarkOpen::Here(_) | MarkOpen::Unnamed(_),
            ) if !shells => self,
            (CommandRuns::Replaced, _) if matches!(token, TokenSeen::Other) => self,
            (CommandRuns::Replaced, _) => MarkOpen::No,
            (CommandRuns::Here, _) => match shells {
                true => MarkOpen::Here(TokenSeen::Shells),
                false => MarkOpen::No,
            },
            (CommandRuns::Nothing, MarkOpen::No | MarkOpen::Here(_) | MarkOpen::Unnamed(_)) => {
                match shells {
                    true => MarkOpen::Unnamed(TokenSeen::Shells),
                    false => self,
                }
            }
            (CommandRuns::Nothing, MarkOpen::Remote(_)) => self,
        }
    }

    /// **Whether a mark carrying `token` may end what is open**: the
    /// shell's own token ends anything open; a remote command opened while
    /// no token was held ends at any mark, as a shell marking its own
    /// prompts without one needs.
    pub const fn ends(self, token: TokenSeen) -> bool {
        match self {
            MarkOpen::No => false,
            MarkOpen::Remote(TokenSeen::Absent) => true,
            MarkOpen::Remote(_) | MarkOpen::Here(_) | MarkOpen::Unnamed(_) => {
                matches!(token, TokenSeen::Shells)
            }
        }
    }
}

/// **How far back a mark's introducer is from its terminator**: from the
/// `ESC` of `ESC ]`, or the `9D`, to the byte that ends it, saturating at
/// `u16::MAX`: a mark that long is that long, and nothing else. A mark
/// begun before the read is [`MarkBack::Before`], not a length.
///
/// ```compile_fail,E0423
/// let _ = baud_domain::MarkLen(3);
/// ```
///
/// No length stands for "before the read":
///
/// ```compile_fail,E0599
/// let _: baud_domain::MarkBack = baud_domain::MarkLen::BEFORE;
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{BytePos, MarkLen};
/// let at = BytePos::from_index;
/// assert_eq!(MarkLen::between(at(2), at(9)).index(), 7);
/// assert_eq!(MarkLen::between(at(0), at(70_000)).index(), usize::from(u16::MAX));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MarkLen(u16);

impl MarkLen {
    /// From `from` to `to`, saturating at `u16::MAX` and at 0; a `from`
    /// past `to` is never asked here, [`MarkBack::of`] reading it as
    /// [`MarkBack::Before`].
    pub fn between(from: crate::BytePos, to: crate::BytePos) -> MarkLen {
        let d = to.index().saturating_sub(from.index());
        MarkLen(u16::try_from(d).unwrap_or(u16::MAX))
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// **Where a mark's introducer is**: this far back from its terminator,
/// in the read ([`MarkLen`]); or before the read began, so every byte of
/// the read up to the terminator is the mark's.
///
/// ```
/// use baud_domain::{BytePos, MarkBack, MarkLen};
/// let at = BytePos::from_index;
/// assert_eq!(MarkBack::of(Some(at(2)), at(9)), MarkBack::Within(MarkLen::between(at(2), at(9))));
/// assert_eq!(MarkBack::of(Some(at(9)), at(2)), MarkBack::Before);
/// assert_eq!(MarkBack::of(None, at(2)), MarkBack::Before);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkBack {
    Within(MarkLen),
    Before,
}

impl MarkBack {
    /// A mark ending at `to` and introduced at `from`, none when that was
    /// before the read; a `from` past `to` is before it too.
    pub fn of(from: Option<crate::BytePos>, to: crate::BytePos) -> MarkBack {
        match from {
            Some(f) if f.index() <= to.index() => MarkBack::Within(MarkLen::between(f, to)),
            Some(_) | None => MarkBack::Before,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(rest: &[u8]) -> PromptOptions<'_> {
        match Semantic::parse(rest) {
            Semantic::Command(_, o) => o,
            other => panic!("{other:?}"),
        }
    }

    /// **Every option is read, not only the first `k=`.**
    #[test]
    fn every_option_of_a_is_read() {
        let o = opts(b"A;aid=14;redraw=last;cl=m;k=s;click_events=2;special_key=1;move-keys=h,l");
        assert_eq!(o.kind, PromptKind::Secondary);
        assert_eq!(o.redraw, Some(Redraw::LastLine));
        assert_eq!(o.click, Some(ClickToMove::Line));
        assert_eq!(o.click_events, Some(ClickEvents::Relative));
        assert_eq!(o.special_key, Some(SpecialKey::Special));
        assert_eq!(o.move_keys, Some(&b"h,l"[..]));
        assert_eq!(o.aid, Aid::of(b"14"));
        let bare = opts(b"A");
        assert_eq!(
            (bare.kind, bare.redraw, bare.aid),
            (PromptKind::Initial, None, Aid::EMPTY)
        );
    }

    /// "A terminal must ignore an option whose option-name it doesn't
    /// recognize", and a malformed value names nothing.
    #[test]
    fn an_unknown_or_malformed_option_is_ignored() {
        let o = opts(b"N;frob=1;redraw=2;click_events=x;k=q;cl=;bare");
        assert_eq!(o.kind, PromptKind::Initial);
        assert_eq!((o.redraw, o.click_events, o.click), (None, None, None));
        assert!(matches!(
            Semantic::parse(b"N"),
            Semantic::Command(Opener::NewCommand, _)
        ));
    }

    #[test]
    fn the_letters_are_the_proposals() {
        assert_eq!(Semantic::parse(b"L"), Semantic::FreshLine);
        assert_eq!(
            Semantic::parse(b"P;k=r"),
            Semantic::Prompt(PromptKind::Right)
        );
        assert_eq!(Semantic::parse(b"B"), Semantic::Input(InputEnd::AtMark));
        assert_eq!(
            Semantic::parse(b"I"),
            Semantic::Input(InputEnd::AtEndOfLine)
        );
        assert!(matches!(
            Semantic::parse(b"C;cmdline_url=ls"),
            Semantic::OutputStart(c) if c.runs() == CommandRuns::Here
        ));
        assert_eq!(Semantic::parse(b"Z"), Semantic::Other);
        assert_eq!(Semantic::parse(b""), Semantic::Other);
    }

    fn status(rest: &[u8], ran: Ran) -> ExitStatus {
        match Semantic::parse(rest) {
            Semantic::CommandDone(d) => d.status(ran),
            other => panic!("{other:?}"),
        }
    }

    /// `err=` takes precedence, as the text says, and the positional code
    /// is any integer.
    #[test]
    fn d_reads_its_code_and_err_gives_way_to_nothing() {
        let ran = Ran::Output;
        assert_eq!(status(b"D;0", ran), ExitStatus::Exited(ExitCode::of(0)));
        assert_eq!(
            status(b"D;-3;aid=7", ran),
            ExitStatus::Exited(ExitCode::of(-3))
        );
        assert_eq!(status(b"D;0;err=", ran), ExitStatus::Succeeded);
        assert_eq!(status(b"D;1;err=", ran), ExitStatus::Succeeded);
        assert_eq!(
            status(b"D;0;err=0", ran),
            ExitStatus::Failed(Some(ExitCode::of(0)))
        );
        assert_eq!(status(b"D;;err=CANCEL", ran), ExitStatus::Cancelled);
        assert_eq!(status(b"D;err=ENOENT", ran), ExitStatus::Failed(None));
        assert_eq!(status(b"D;bogus", ran), ExitStatus::Unreported);
        assert_eq!(status(b"D", ran), ExitStatus::Unreported);
        assert_eq!(status(b"D;99999999999", ran), ExitStatus::Unreported);
        // A `D` with no `C` before it is iTerm2's abort, its code ignored.
        assert_eq!(status(b"D;0", Ran::NoOutput), ExitStatus::Cancelled);
        match Semantic::parse(b"D;2;aid=x") {
            Semantic::CommandDone(d) => assert_eq!(d.aid, Aid::of(b"x")),
            other => panic!("{other:?}"),
        }
    }

    /// The options of a `C` mark naming `line` as `cmdline_url=`, each
    /// byte not a letter, digit, `/`, `-` or `.` escaped as `%XX`.
    fn url_of(line: &str, into: &mut [u8; 256]) -> usize {
        let mut n = 0;
        let mut put = |b: u8| {
            into[n] = b;
            n += 1;
        };
        b"cmdline_url=".iter().for_each(|&b| put(b));
        for b in line.bytes() {
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'/' | b'-' | b'.' => put(b),
                _ => {
                    let hex = b"0123456789ABCDEF";
                    put(b'%');
                    put(hex[usize::from(b >> 4)]);
                    put(hex[usize::from(b & 15)]);
                }
            }
        }
        n
    }

    /// **The program is the first word past assignments, the prefixes and
    /// their options, unquoted as the shell would**, its last path
    /// component checked against [`ELSEWHERE`]: every shape of `ssh` and
    /// `mosh` is `Elsewhere`, the same shapes of `ls` are not, read as
    /// `cmdline_url=`; `exec` of a local program is `Replaced`; nothing
    /// named is `Nothing`. Named break: take the line's first word as it is
    /// -- the prefixed and quoted shapes read as `sudo`, `'ssh'` and the
    /// like.
    #[test]
    fn the_program_is_the_first_word_past_prefixes_and_quotes() {
        let shapes = [
            ("ssh host", "ls host"),
            ("  ssh\thost", "  ls\thost"),
            ("/usr/bin/ssh -p 22 host", "/usr/bin/ls -p 22 host"),
            ("'ssh' host", "'ls' host"),
            ("\"mosh\" host", "\"ls\" host"),
            ("\\ssh host", "\\ls host"),
            ("X=1 Y='a b' ssh host", "X=1 Y='a b' ls host"),
            ("command ssh host", "command ls host"),
            ("env -i ssh host", "env -i ls host"),
            ("nice -10 ssh host", "nice -10 ls host"),
            ("time ssh host", "time ls host"),
            ("sudo -E ssh host", "sudo -E ls host"),
            ("ssh; ls", "ls; ssh"),
            ("ssh|cat", "ls|ssh"),
            ("$'ss\\x68' h", "$'l\\163' h"),
            (".ssh-wrapped h", ".ls-wrapped h"),
        ];
        let mut buf = [0u8; 256];
        for (remote, here) in shapes {
            for (line, want) in [(remote, CommandRuns::Elsewhere), (here, CommandRuns::Here)] {
                let n = url_of(line, &mut buf);
                assert_eq!(CommandRuns::of(&buf[..n]), want, "{line:?}");
            }
        }
        assert_eq!(
            CommandRuns::of(b"cmdline=exec\\ zsh"),
            CommandRuns::Replaced
        );
        assert_eq!(
            CommandRuns::of(b"cmdline=exec\\ ssh\\ h"),
            CommandRuns::Elsewhere
        );
        for nothing in [
            &b""[..],
            b"aid=1",
            b"cmdline=",
            b"cmdline=''",
            b"cmdline_url=",
            b"cmdline=X=1",
        ] {
            assert_eq!(
                CommandRuns::of(nothing),
                CommandRuns::Nothing,
                "{nothing:?}"
            );
        }
        assert_eq!(
            CommandRuns::of(b"aid=1;cmdline=ssh;cmdline=ls"),
            CommandRuns::Elsewhere,
            "the first"
        );
    }

    /// **A command line cut at the bound names only from whole words**:
    /// clipped, a program word that runs to the cut names nothing, one
    /// ended before it is named, and an option after the last `;` is the
    /// cut one. Named break: read clipped options as whole -- the cut path
    /// reads as a program here.
    #[test]
    fn a_clipped_command_line_names_only_whole_words() {
        assert_eq!(
            CommandRuns::of_clipped(b"cmdline=/usr/a/ss"),
            CommandRuns::Nothing
        );
        assert_eq!(
            CommandRuns::of(b"cmdline=/usr/a/ss"),
            CommandRuns::Here,
            "the control"
        );
        assert_eq!(
            CommandRuns::of_clipped(b"cmdline=ssh\\ hos"),
            CommandRuns::Elsewhere
        );
        assert_eq!(
            CommandRuns::of_clipped(b"cmdline=ls;aid=12"),
            CommandRuns::Here
        );
    }

    /// **The steps of what is open, mark by mark**: a remote program's `C`
    /// opens remote and its `D` cannot end it, the shell's `D` can; the
    /// shell's `C` naming a program here opens here and one naming nothing
    /// opens unnamed, a program's `C` opens neither; a remote command opened
    /// while no token is held ends at any `D`; the shell's `exec` ends
    /// everything, another token's `exec` nothing. Named break: let any `D`
    /// end a remote command (`MarkOpen::ends` true for every `Remote`) --
    /// the program's `D` closes it.
    #[test]
    fn what_is_open_steps_only_on_the_shells_token() {
        use CommandRuns::{Elsewhere, Here, Nothing, Replaced};
        use TokenSeen::{Absent, Other, Shells};
        let c = crate::StreamByte::from_stream(b'C');
        let d = crate::StreamByte::from_stream(b'D');
        let a = crate::StreamByte::from_stream(b'A');
        let remote = MarkOpen::No.step(c, Other, Elsewhere);
        assert_eq!(remote, MarkOpen::Remote(Other));
        assert_eq!(remote.step(d, Other, Nothing), remote, "a program's D");
        assert_eq!(remote.step(c, Other, Here), remote, "a program's C for ls");
        assert_eq!(remote.step(a, Shells, Nothing), remote, "an A");
        assert_eq!(
            remote.step(d, Shells, Nothing),
            MarkOpen::No,
            "the shell's D"
        );
        assert_eq!(
            remote.step(c, Shells, Here),
            MarkOpen::Here(Shells),
            "the shell's next"
        );
        assert_eq!(
            MarkOpen::No.step(c, Shells, Nothing),
            MarkOpen::Unnamed(Shells)
        );
        assert_eq!(MarkOpen::No.step(c, Other, Nothing), MarkOpen::No);
        assert_eq!(MarkOpen::No.step(c, Other, Here), MarkOpen::No);
        let fish = MarkOpen::No.step(c, Absent, Elsewhere);
        assert_eq!(fish.step(d, Absent, Nothing), MarkOpen::No, "no token held");
        assert_eq!(fish.step(c, Absent, Here), MarkOpen::No, "the next command");
        let here = MarkOpen::Here(Shells);
        assert_eq!(
            here.step(c, Shells, Replaced),
            MarkOpen::No,
            "the shell's exec"
        );
        assert_eq!(here.step(c, Other, Replaced), here, "another's exec");
        assert_eq!(remote.step(c, Shells, Elsewhere), MarkOpen::Remote(Shells));
        assert_eq!(
            MarkOpen::Remote(Shells).step(c, Other, Elsewhere),
            MarkOpen::Remote(Shells),
            "a program's ssh keeps the shell's"
        );
    }
}
