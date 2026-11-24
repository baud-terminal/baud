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
    /// `C`: "End of input, and start of output."
    OutputStart,
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
    /// The mark named by the bytes after `133;`.
    pub fn parse(rest: &'a [u8]) -> Semantic<'a> {
        let (letter, fields) = match rest.iter().position(|&b| b == b';') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        match letter {
            b"L" => Semantic::FreshLine,
            b"A" => Semantic::Command(Opener::FreshLineNewPrompt, PromptOptions::of(fields)),
            b"N" => Semantic::Command(Opener::NewCommand, PromptOptions::of(fields)),
            b"P" => Semantic::Prompt(PromptOptions::of(fields).kind),
            b"B" => Semantic::Input(InputEnd::AtMark),
            b"I" => Semantic::Input(InputEnd::AtEndOfLine),
            b"C" => Semantic::OutputStart,
            b"D" => Semantic::CommandDone(done(fields)),
            _ => Semantic::Other,
        }
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
        assert_eq!(Semantic::parse(b"C;cmdline_url=ls"), Semantic::OutputStart);
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
}
