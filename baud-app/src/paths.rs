//! **A file path printed as plain text, underlined at rest only if the file
//! exists**: what the frame's scan finds and draws, pure over the core's cells
//! and what the worker last said, so its tests run on every platform and the
//! `frame` bench compiles this file as it stands. No filesystem call is made
//! here, by anything: whether a path exists is asked of one worker thread
//! (`probe.rs`) and read back from [`Paths`]' results, and a frame never waits
//! for an answer.
//!
//! **A frame reads no row for paths unless one it would draw is kept**
//! ([`Reads`]): a changed row only owes a batch, and the collection that
//! answers it, once the output settles and at most one a second, reads every
//! row of the window then. So output with no path in it pays one compare a
//! cell, not a read of every row a `/` starts a word on.
//!
//! **The forms.** A printed `/`, `~/`, `./` or `../` starts a path, after the start
//! of a line, a space or one of `([{<"'` and `` ` `` or `=`, and never after
//! a `:`, so neither `https://a/b`'s `//a/b` nor `host:/x`'s `/x` is one.
//! So do a relative path with no prefix and a `/` inside, such as
//! `src/app.rs` ([`Form::Bare`]), and a single name holding a `.` not at its
//! end, such as `README.md` or `.zshrc` ([`Form::Name`]), read against the
//! folder a `./` is; a relative path ends before a `=` and at a `:` a `/` or
//! `~/` follows, so a prefixed path after either is read. Any single word
//! ([`Form::Word`]), a `:line:col` suffix and a quoted path with spaces are
//! each a [`Forms`] switch, off until `[link.path]` turns it on. A path
//! runs to the next space, quote, `<`, `>` or `|`; trailing `.,;:!?*` and a
//! closing bracket it did not open are prose, as a URI's are (`link.rs`).
//!
//! **A name is a link only where its file is.** A version number, a domain
//! or a dotted identifier in a log is a name too, and the worker's lookup
//! misses it, so it gets no line: the existence check is the guard, and a
//! name is never struck at rest. A batch asks at most [`NAMES_MAX`] of them,
//! after every other path the screen holds, and `[link.path] names =
//! "hover"` ([`NameLook`]) draws them only under the pointer.
//!
//! **What a path is resolved against**: `/` this machine, `~/` the home
//! folder `HOME` named when baud started, `./` the folder the shell last
//! reported with `OSC 7`. **baud keeps one reported folder, not one per
//! prompt** (`Term::working_directory`), so a `./` path gets a line only on a
//! row at or below the line where that report took effect ([`Dir::from`]),
//! and only while it names this machine by name: a report with no host or
//! `localhost` names no machine (`[[source: rfc8089]]` § 2 calls both local,
//! and a program on another machine can send either). A row printed before
//! the last `cd` gets no `./` line, and two reports in one read cannot be told
//! apart: the rows that read wrote are above the line, so they get none.
//!
//! **On the alternate screen a relative path is read against the folder of
//! the program in front** ([`ProgramDir`]), since a full-screen program has
//! a folder of its own and reports none: the worker reads it for the
//! foreground group, on its thread, while that group is named local, and it
//! goes with the alternate screen and when another group comes in front
//! (`probe.rs`). A document's links are relative to the document, not the
//! program's folder; the two are one for `hx README.md`, and where they are
//! not, a path that is not there gets no line, never a line on another file.
//!
//! **Output not known to be on this machine gets no mark at all.** The
//! worker names the program in front before it looks anything up: under
//! `ssh` and its kin nothing is looked up, and the rows written up to that
//! check get no mark after it ends ([`Paths::set_front`]); under a terminal
//! multiplexer a `./` gets none, since baud cannot see a pane's folder, and
//! `/` and `~/` mean "on this Mac", which the bar says.
//!
//! **What a line means**: the worker found the file there, by a walk that
//! never steps onto a network or automounted file system (`probe.rs`), and a
//! click reveals it in Finder after asking again, through `admit` as a
//! `file://` URI ([`file_uri`]). It is a hint, never a promise: the file can
//! go between the look and the click, and the click says so.
//!
//! **An overlay and nothing else**: nothing here writes a cell, so no query
//! of the core's -- `DECRQCRA` among them -- answers differently with paths
//! shown, and no program can learn from it whether a file exists.
//!
//! **No path is logged.** Paths are private: what the worker does is counted
//! (`report.rs`), never named.
//!
//! **What is struck, and only that** (`[link] missing = "strike"`): a strike
//! says "this would open nothing", so it comes only from what the worker
//! found, never from a guess. Three things, and nothing else:
//!
//! * an `OSC 8` `file://` link whose host names this machine by name
//!   ([`Paths::set_names`]), whose file the walk found gone -- `ENOENT` or
//!   `ENOTDIR` on a local file system ([`Probe::Missing`]) -- on output known
//!   to be this machine's. `file:///x` and `file://localhost/x` name no
//!   machine (`[[source: rfc8089]]` § 2 calls both local, and a program on
//!   another machine can send either, through a multiplexer's pane), so
//!   they are struck only under the gesture's modifier, where the bar says
//!   the file is not on this machine;
//! * a link whose kind has no app ([`Handler::Missing`]), asked by kind at a
//!   fixed address ([`address`]), never with a program's bytes;
//! * a command's program not found ([`Program::Command`]), the hook the
//!   `[link.*]` rules' `command` uses.
//!
//! A printed path is struck only under the modifier (`link.rs`), never at
//! rest: baud cannot see whether a pane under a multiplexer, a container or
//! an `ssh` printed it. Everything baud cannot tell -- an error other than
//! those two, a network or automounted file system, a hang, a host it does
//! not know, an answer not come yet -- leaves no mark. A `file://` link
//! naming another host is never looked up: `admit` refuses it, and it is
//! drawn as no link, as before. With `paths = "off"` there is no worker, so
//! nothing is asked and nothing struck.
#![forbid(unsafe_code)]

use core::ops::Range;
use std::collections::{HashMap, HashSet};

use baud_core::{FileHost, Screen, Term};
use baud_domain::{Cols, LinkEpoch, LinkId, Rows};
use baud_render::{AbsLine, Scrolled, Tally, row_of};

use crate::lined::{At, Drawn, Key, Lines, Mark, Reader, Run, Text, Uris};
use crate::link::{Edge, Ix, Joins, MAX, Scheme, Target, Unit, admit_link};
use crate::rules::{NO_RULES, Rules};

/// What the bar calls this machine.
#[cfg(target_os = "macos")]
pub const MACHINE: &str = "this Mac";
#[cfg(not(target_os = "macos"))]
pub const MACHINE: &str = "this computer";

/// The most paths one batch asks about. A screen with more asks about the
/// rest in the batches after it, at most one a second (`probe::Pace`).
pub const BATCH_MAX: usize = 512;

/// **The most single names one batch asks about**, of [`BATCH_MAX`]: a
/// name or word ([`Form::Name`], [`Form::Word`]) takes a slot only after
/// every prefixed or relative path the screen holds has one, and never
/// more than these, so a screen of dotted identifiers neither starves a
/// `/` or `./` path of its answer nor asks more than this many a batch.
pub const NAMES_MAX: usize = 256;

/// A switch: a form read, a kind of link shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Turned {
    #[default]
    On,
    Off,
}

/// **How a printed path starts**, and so what it is resolved against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// `/`: this machine.
    Absolute,
    /// `~/`: the home folder.
    Home,
    /// `./`: the reported folder.
    Dot,
    /// `../`: the reported folder's parent.
    Parent,
    /// `src/app.rs`, no prefix and a `/` inside: the reported folder.
    Bare,
    /// `README.md`, `.zshrc`: one component, no `/`, holding a `.` not at its
    /// end, read against the reported folder.
    Name,
    /// `Makefile`, `src`: any one component, read against the reported
    /// folder. Off by default.
    Word,
}

impl Form {
    /// Whether it is read against the reported folder, which a multiplexer
    /// hides.
    pub fn relative(self) -> bool {
        matches!(
            self,
            Form::Dot | Form::Parent | Form::Bare | Form::Name | Form::Word
        )
    }

    /// Whether it is a single name, which [`NameLook`] governs and
    /// [`NAMES_MAX`] bounds.
    fn single(self) -> bool {
        matches!(self, Form::Name | Form::Word)
    }
}

/// **How a single name the worker found is drawn** (`[link.path] names`):
/// governs [`Form::Name`] and [`Form::Word`] only; every other form keeps
/// its line at rest.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NameLook {
    /// Underlined at rest, as every other link is.
    #[default]
    Line,
    /// No line at rest: lit only under the pointer, as the look a click
    /// would open shows (`link::Hover`).
    Hover,
}

/// **Which forms are read**: the four printed prefixes, relative paths and
/// single names holding a `.` by default, and the rest only once
/// `[link.path]` names them (`rules.rs`). Nothing else changes when one is
/// turned on: the worker, the mount check and the click are the same for
/// every form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Forms {
    pub absolute: Turned,
    pub home: Turned,
    pub dot: Turned,
    pub parent: Turned,
    pub bare: Turned,
    pub name: Turned,
    pub word: Turned,
    /// How a single name found is drawn.
    pub names: NameLook,
    /// A `:line` or `:line:col` suffix, kept under the line and left out of
    /// the path looked up.
    pub line_col: Turned,
    /// Spaces inside a path that a `"` or `'` opened and closes.
    pub spaces: Turned,
}

impl Forms {
    pub const DEFAULT: Forms = Forms {
        absolute: Turned::On,
        home: Turned::On,
        dot: Turned::On,
        parent: Turned::On,
        bare: Turned::On,
        name: Turned::On,
        word: Turned::Off,
        names: NameLook::Line,
        line_col: Turned::Off,
        spaces: Turned::Off,
    };

    fn of(&self, form: Form) -> Turned {
        match form {
            Form::Absolute => self.absolute,
            Form::Home => self.home,
            Form::Dot => self.dot,
            Form::Parent => self.parent,
            Form::Bare => self.bare,
            Form::Name => self.name,
            Form::Word => self.word,
        }
    }

    /// Whether a path with no prefix is read at all, in any of its forms.
    fn unprefixed(&self) -> bool {
        [self.bare, self.name, self.word].contains(&Turned::On)
    }

    /// Whether a found path of `form` is underlined at rest.
    fn at_rest(&self, form: Form) -> bool {
        !(form.single() && self.names == NameLook::Hover)
    }
}

impl Default for Forms {
    fn default() -> Forms {
        Forms::DEFAULT
    }
}

/// **What the worker found**, the one answer the line and the strike read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Probe {
    /// There, on a local file system.
    Exists,
    /// Not there: a component missing (`ENOENT`), or a file where a folder
    /// was named (`ENOTDIR`), on a local file system.
    Missing,
    /// Anything baud cannot stand behind: a network or automounted file
    /// system on the way, a folder it may not read, too many links, an
    /// error. No mark either way.
    Unknown,
}

/// **What a program for a link is**, as the worker last found it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Handler {
    /// Not asked yet, or not one baud can tell: never struck.
    #[default]
    Unknown,
    Found,
    /// None: the system names no app for the kind, or no file of the
    /// command's name is on its path.
    Missing,
}

/// **What the worker is asked to find a program for**: from a fixed list,
/// the config's and never the program's output -- the schemes `admit`
/// opens, and the `[link.*]` rules' `command`s.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Program {
    /// The app the system would open a kind of link with, asked at
    /// [`address`]'s fixed address.
    Scheme(Scheme),
    /// A rule's `command[0]`, as its reader spells it once read, looked up
    /// as `execvp` would, a component at a time and never past a network or
    /// automounted file system (`probe::command`), for each `[link.NAME]`
    /// rule that runs a command (`config::Link::programs`).
    Command(String),
}

/// **The fixed address a kind of link is asked about**: its scheme and a
/// name under `.invalid`, which is for "domain names that are sure to be
/// invalid" (`[[source: rfc2606]]` § 2), so no app claims it for itself and
/// no byte a program printed reaches the lookup. None for `file`, which is
/// revealed in the file manager and asked about nothing, and for a scheme a
/// `[link.*]` rule added, which no fixed address stands for.
pub fn address(scheme: Scheme) -> Option<&'static str> {
    match scheme {
        Scheme::Http => Some("http://example.invalid/"),
        Scheme::Https => Some("https://example.invalid/"),
        Scheme::Mailto => Some("mailto:nobody@example.invalid"),
        Scheme::File | Scheme::Added => None,
    }
}

/// **Why a link `admit` lets through would open nothing**, as the worker
/// last found: for a struck link's bar and for a click, which asks again.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Gone {
    /// Its file is gone from this machine: the path, as the click asks it.
    File(Vec<u8>),
    /// No app for its kind.
    NoApp(Scheme),
    /// No program for the command its `[link.NAME]` rule runs.
    NoCommand,
}

/// What a `file://` link's host says of where its file is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Named {
    /// This machine, by one of its names.
    ThisMachine,
    /// No machine: empty, or `localhost`.
    NoMachine,
}

/// **The file a `file:` URI names on this machine**, percent-decoded,
/// and whether its host named this machine: read by RFC 8089's grammar
/// (`baud_domain::FileUri`), as the link policy reads it. None for another
/// host, a URI off the grammar -- a query or a fragment, a `%` without two
/// hex digits, a byte the grammar does not allow, `file:////srv/x` -- a
/// path that decodes to a NUL or to bytes that are not UTF-8, or a control
/// or bidirectional control in the name: none of which is looked up, so
/// none is struck.
pub fn file_path(uri: &[u8], names: &crate::link::MachineNames) -> Option<(Named, Vec<u8>)> {
    let baud_domain::DirectoryReport::File(f) = baud_domain::DirectoryReport::parse(uri) else {
        return None;
    };
    let named = match f.host {
        baud_domain::FileHost::Local => Named::NoMachine,
        baud_domain::FileHost::Named(h) if names.is_this(h) => Named::ThisMachine,
        baud_domain::FileHost::Named(_) => return None,
    };
    let mut out = vec![0; f.encoded_path().len()];
    let n = f.decode_path(&mut out)?.len();
    out.truncate(n);
    let text = core::str::from_utf8(&out).ok()?;
    if text.chars().any(|c| c.is_control() || bidi_control(c)) {
        return None;
    }
    Some((named, out))
}

/// A path the worker is asked about, as resolved, and the last line it was
/// printed on: a path printed only on rows a program not on this machine
/// wrote is not looked up (`probe.rs`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Candidate {
    pub path: Vec<u8>,
    pub form: Form,
    pub line: AbsLine,
}

/// **A path in a line's text**, before it is resolved.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Token {
    /// Its units, the suffix a `:line:col` adds included.
    pub span: Range<usize>,
    /// The units of the path itself.
    pub path: Range<usize>,
    pub form: Form,
}

/// A character that may start a path after it: none (the start of the
/// line, an empty cell), a space, an opening bracket or quote, or `=`.
fn opens(u: Unit) -> bool {
    match u {
        Unit::Gap => true,
        Unit::Marked => false,
        Unit::Char(c) => c.is_whitespace() || "([{<\"'`=".contains(c),
    }
}

/// The ASCII bytes of [`opens`], for the scan's tail, where a byte outside
/// ASCII reads as zero and so as an opener: the line is read, and the text
/// decides.
const OPENS: &[u8] = b"\0 \t([{<\"'`=";

/// A bidirectional control, which a path is refused whole for: it can make
/// a name read as another (`[[source: uax9-tr9-52]]`, the explicit
/// formatting characters and the marks).
fn bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// What a unit is to a path: part of it, its end, or a reason to refuse it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Part {
    In,
    Space,
    End,
    Refuse,
}

fn part(u: Unit) -> Part {
    match u {
        Unit::Gap => Part::End,
        // Its character is not in the unit, so its bytes cannot be named.
        Unit::Marked => Part::Refuse,
        Unit::Char(c) if c.is_control() || bidi_control(c) => Part::Refuse,
        Unit::Char(c) if c.is_whitespace() => Part::Space,
        Unit::Char(c) if "\"'`<>|".contains(c) => Part::End,
        Unit::Char(_) => Part::In,
    }
}

fn is(u: Option<&Unit>, c: char) -> bool {
    matches!(u, Some(Unit::Char(x)) if *x == c)
}

/// **Whether `tail`, the last eight ASCII bytes read, may end a path's
/// start**, for the scan's one compare a cell: a `/` after an opener, `~`, `.`
/// or `..`; any `/` once bare paths are read; a `.` before a character a
/// path may hold once names are read, which every name holds (its `.` is
/// not at its end); and a name's first character after an opener once words
/// are read. A byte outside ASCII, and an empty cell, read as zero, which
/// may be either, so zero is let through and the text decides.
pub fn starts(tail: [u8; 8], forms: &Forms) -> bool {
    // The newest byte last: `at(1)` is the one before the `/`.
    let at = |k: usize| tail[7 - k];
    let opens = |k: usize| OPENS.contains(&at(k));
    // A byte a name may hold after its `.`, or zero, which may be one: never
    // a `/`, which makes it a `./` or `../`.
    let held = |b: u8| b == 0 || (b.is_ascii_graphic() && !b"\"'`<>|/".contains(&b));
    if forms.name == Turned::On && at(1) == b'.' && held(at(0)) {
        return true;
    }
    if forms.word == Turned::On
        && (at(0) == 0 || at(0).is_ascii_alphanumeric() || b"_.-@+".contains(&at(0)))
        && opens(1)
    {
        return true;
    }
    if at(0) != b'/' {
        return false;
    }
    if forms.bare == Turned::On {
        return true;
    }
    (forms.absolute == Turned::On && opens(1))
        || (forms.home == Turned::On && at(1) == b'~' && opens(2))
        || (forms.dot == Turned::On && at(1) == b'.' && opens(2))
        || (forms.parent == Turned::On && at(1) == b'.' && at(2) == b'.' && opens(3))
}

/// **What a relative path with no prefix may start with**: a letter, a
/// digit, or one of `_.-@+`, as a file's name does -- never the `!`, `[` or
/// `(` of the Markdown around one, whose path starts after them.
fn names(u: &Unit) -> bool {
    matches!(u, Unit::Char(c) if c.is_alphanumeric() || "_.-@+".contains(*c))
}

/// The form of a path starting at the first of `rest`, after `before`, the
/// unit before it (none at the line's start): none when nothing that `forms`
/// reads starts there.
fn start_at(before: Option<Unit>, rest: &[Unit], forms: &Forms) -> Option<Form> {
    if before.is_some_and(|b| !opens(b)) {
        return None;
    }
    let u = |k: usize| rest.get(k);
    let first = |k: usize| u(k).is_some_and(|&x| part(x) == Part::In && !is(Some(&x), '/'));
    let form = if is(u(0), '/') && first(1) {
        Form::Absolute
    } else if is(u(0), '~') && is(u(1), '/') && u(2).is_some_and(|&x| part(x) == Part::In) {
        Form::Home
    } else if is(u(0), '.') && is(u(1), '/') && first(2) {
        Form::Dot
    } else if is(u(0), '.') && is(u(1), '.') && is(u(2), '/') && first(3) {
        Form::Parent
    } else if u(0).is_some_and(names) {
        // No prefix: which of its forms it is, [`unprefixed_form`] says once
        // its end is known.
        return forms.unprefixed().then_some(Form::Bare);
    } else {
        return None;
    };
    (forms.of(form) == Turned::On).then_some(form)
}

/// **What a path with no prefix is, its text known**: a relative path with
/// a `/` inside, a name holding a `.` (never at its end, since the trailing
/// prose is cut first), or a word; none with a `:`, since `host:path`
/// names another machine and `http:` is a scheme, and none of only `.` and
/// `/`, since a lone `./` or `../` is a prefix with no path.
fn unprefixed_form(text: &[Unit]) -> Option<Form> {
    if text.iter().any(|u| is(Some(u), ':'))
        || text.iter().all(|u| is(Some(u), '.') || is(Some(u), '/'))
    {
        return None;
    }
    Some(if text.iter().any(|u| is(Some(u), '/')) {
        Form::Bare
    } else if text.iter().any(|u| is(Some(u), '.')) {
        Form::Name
    } else {
        Form::Word
    })
}

/// **Whether `u` is a closing bracket `run` did not open**: as many of its
/// closers in `run` as openers, or more.
fn unopened(run: &[Unit], u: Unit) -> bool {
    let Some(open) = (match u {
        Unit::Char(')') => Some('('),
        Unit::Char(']') => Some('['),
        Unit::Char('}') => Some('{'),
        _ => None,
    }) else {
        return false;
    };
    let count = |c: char| run.iter().filter(|x| is(Some(x), c)).count();
    count(open)
        <= count(match u {
            Unit::Char(c) => c,
            _ => open,
        })
}

/// **The paths in a line's text**, each once, in order: from a start
/// [`start_at`] reads, over path characters, to the first that ends one, its
/// trailing prose cut off. A path the walk's end touches, one past `MAX`
/// characters, or one with a character it must not hold is none. One with
/// no prefix starts at a name's character ([`names`]) and ends at a closing
/// bracket it did not open ([`unopened`]), so a Markdown link's path is read
/// without the brackets around it; it is a relative path, a name or a word
/// ([`unprefixed_form`]), and none with a `:` (`host:path` names another
/// machine). One whose form is not read leaves the text after its first
/// character to be read again, so `--file=/tmp/a` still holds `/tmp/a`.
pub fn tokens(units: &[Unit], (head, tail): (Edge, Edge), forms: &Forms, out: &mut Vec<Token>) {
    out.clear();
    let n = units.len();
    let mut i = 0;
    // Where a relative path ended at a list's `:`, the path after it may
    // start there, as after an opener.
    let mut listed = None;
    while i < n {
        let before = match listed {
            Some(k) if k == i => None,
            _ => i.checked_sub(1).map(|k| units[k]),
        };
        let Some(form) = start_at(before, &units[i..], forms) else {
            i += 1;
            continue;
        };
        // A quote just before it, when spaces are read: the path may run to
        // the matching quote, and only there.
        let quote = match (forms.spaces, i.checked_sub(1).map(|k| units[k])) {
            (Turned::On, Some(Unit::Char(q @ ('"' | '\'')))) => Some(q),
            _ => None,
        };
        let mut end = i;
        let mut refused = false;
        let mut list = None;
        // A relative path with no prefix ends at a closing bracket it did
        // not open: `[t](docs/a.md)`'s `]`, whose wrapper's path the scan
        // reads from there; before a `=`, which ends an option's or a key's
        // name, so a prefixed path after it is read (`--file=/tmp/a`); and
        // at a `:` a `/` or `~/` follows once it holds a `/`, which
        // separates a list's paths (`a/b:/c/d`).
        while end < n {
            match part(units[end]) {
                Part::In if form == Form::Bare && unopened(&units[i..end], units[end]) => break,
                Part::In if form == Form::Bare && is(units.get(end), '=') => break,
                Part::In if form == Form::Bare && lists(&units[i..end], &units[end..]) => {
                    list = Some(end + 1);
                    break;
                }
                Part::In => end += 1,
                Part::Space if quote.is_some() => end += 1,
                Part::Refuse => {
                    refused = true;
                    end += 1;
                }
                Part::Space | Part::End => break,
            }
        }
        if let Some(q) = quote {
            // Spaces only when the quote closes it; otherwise the path is
            // what it would be without them.
            if !is(units.get(end), q) {
                end = (i..end)
                    .find(|&k| part(units[k]) == Part::Space)
                    .unwrap_or(end);
            }
        }
        let next = end.max(i + 1);
        let touches = (i == 0 && head == Edge::Cut) || (end == n && tail == Edge::Cut);
        if refused || touches || end - i > MAX {
            // One with no prefix may hold a prefixed one after an opener.
            i = match form {
                Form::Bare => i + 1,
                _ => next,
            };
            continue;
        }
        let end = i + trimmed(&units[i..end]).len();
        let path_end = match forms.line_col {
            Turned::On => i + without_line_col(&units[i..end]).len(),
            Turned::Off => end,
        };
        let text = &units[i..path_end];
        // `start_at` reads every path with no prefix as `Bare` until its end
        // is known: what it is decides whether it is read.
        let form = match form {
            Form::Bare => match unprefixed_form(text).filter(|f| forms.of(*f) == Turned::On) {
                Some(f) => {
                    if f == Form::Bare {
                        listed = list;
                    }
                    f
                }
                None => {
                    i += 1;
                    continue;
                }
            },
            f => f,
        };
        let prefix = match form {
            Form::Absolute | Form::Bare => 1,
            Form::Home | Form::Dot => 2,
            Form::Parent => 3,
            Form::Name | Form::Word => 0,
        };
        if path_end - i > prefix {
            out.push(Token {
                span: i..end,
                path: i..path_end,
                form,
            });
        }
        i = next;
    }
}

/// **Whether the `:` first in `rest` separates a list's paths**, after
/// `run`, a relative path's text so far: `run` holds a `/` and a `/` or `~/`
/// follows the `:`. `host:/x` names another machine and `src/app.rs:12`
/// keeps its line, so neither is a list.
fn lists(run: &[Unit], rest: &[Unit]) -> bool {
    let u = |k: usize| rest.get(k);
    is(u(0), ':')
        && run.iter().any(|x| is(Some(x), '/'))
        && (is(u(1), '/') || (is(u(1), '~') && is(u(2), '/')))
}

/// The path in `t`, its trailing prose cut: `.,;:!?*'` and a closing bracket
/// it did not open, as `find` cuts a URI's (`link.rs`).
fn trimmed(t: &[Unit]) -> &[Unit] {
    let mut end = t.len();
    while end > 0 {
        let Unit::Char(c) = t[end - 1] else { break };
        let open = match c {
            ')' => '(',
            ']' => '[',
            '}' => '{',
            _ if ".,;:!?*'".contains(c) => {
                end -= 1;
                continue;
            }
            _ => break,
        };
        let opened = t[..end].iter().filter(|u| is(Some(u), open)).count();
        let closed = t[..end].iter().filter(|u| is(Some(u), c)).count();
        if closed > opened {
            end -= 1;
        } else {
            break;
        }
    }
    &t[..end]
}

/// `t` with a `:line` or `:line:col` suffix taken off.
fn without_line_col(t: &[Unit]) -> &[Unit] {
    let mut end = t.len();
    for _ in 0..2 {
        let digits = t[..end]
            .iter()
            .rev()
            .take_while(|u| matches!(u, Unit::Char(c) if c.is_ascii_digit()))
            .count();
        if digits == 0 || digits + 1 >= end || !is(t.get(end - digits - 1), ':') {
            break;
        }
        end -= digits + 1;
    }
    &t[..end]
}

/// **What a path read against the folder adds to it**: a `./` path its
/// rest, `./late` in `/w` naming `/w/late`, since `.` is the folder
/// itself; a `../` or bare path all of it.
fn below_folder(form: Form, text: &[Unit]) -> &[Unit] {
    match form {
        Form::Dot => text.get(2..).unwrap_or(&[]),
        Form::Absolute | Form::Home | Form::Parent | Form::Bare | Form::Name | Form::Word => text,
    }
}

/// The bytes of `units`, UTF-8, as a file system names a file: only
/// [`Unit::Char`] reaches here ([`part`] refuses the rest).
fn push_utf8(units: &[Unit], out: &mut Vec<u8>) {
    let mut b = [0u8; 4];
    for u in units {
        if let Unit::Char(c) = u {
            out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
        }
    }
}

/// **The `file://` URI a click on `path` hands `admit`**: no authority, the
/// path's UTF-8 bytes percent-encoded but for the unreserved characters and
/// `/`, with uppercase hex. RFC 8089's `local-path` is a `path-absolute`
/// (`[[source: rfc8089]]` § 2), § 4 asks for UTF-8 "before percent-encoding
/// is applied", and RFC 3986 makes `unreserved` the set that needs none and
/// uppercase hex the form to produce (`[[source: rfc3986]]` § 2.1, § 2.3).
/// Every byte of the result is printable ASCII, so `admit` reads it as it
/// reads any `file` link, and refuses it past `LINK_MAX`.
pub fn file_uri(path: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity("file://".len() + path.len());
    out.push_str("file://");
    for &b in path {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(char::from(b));
            }
            _ => {
                out.push('%');
                out.push(char::from(HEX[usize::from(b >> 4)]));
                out.push(char::from(HEX[usize::from(b & 0xF)]));
            }
        }
    }
    out
}

/// **Whether a path's URI opens**, without building it: what `admit` answers
/// for [`file_uri`]'s bytes, which are printable ASCII, start `file://` with
/// no host and an absolute path, and so are refused only for their length.
/// The frame asks this of every path it draws, so it allocates nothing; a
/// click goes through `admit` itself, and
/// `opening_is_what_admit_answers` holds the two to one answer.
pub fn opens_as_uri(path: &[u8]) -> bool {
    let unreserved = |b: &u8| b.is_ascii_alphanumeric() || b"-._~/".contains(b);
    let len = "file://".len()
        + path
            .iter()
            .map(|b| if unreserved(b) { 1 } else { 3 })
            .sum::<usize>();
    path.first() == Some(&b'/') && len <= MAX
}

/// **What a batch's answers changed**, in rising order: nothing; only
/// printed paths' answers, which redraw the runs the scan keeps; or what
/// reads every row again: an `OSC 8` link's answer, or the reading of
/// printed paths started over a row read without it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Answered {
    Nothing,
    Printed,
    Everything,
}

/// **The answers' generation**: moved by every change a read of the link
/// under the pointer must see, which comes with no byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Generation(u64);

impl Generation {
    fn next(self) -> Generation {
        Generation(self.0.wrapping_add(1))
    }
}

/// Whether a collection met more than a batch holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Over {
    #[default]
    No,
    Yes,
}

/// **Whether a frame's scan reads paths at all**: only in a collection, and
/// otherwise only while an answer the scan would draw is kept. A frame of
/// output with none spends one compare a cell on paths and reads no row for
/// them; what it holds is read once a batch, at the settle or the ceiling,
/// by the collection -- but for the lines still to be named
/// ([`Reads::Pending`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Reads {
    #[default]
    No,
    /// **Only lines still to be named, and only for whether they hold a
    /// path** ([`Paths::read_pending`]): their naming makes such a path
    /// readable, so the batch naming them is owed for a path
    /// ([`Owes::Path`]), which the shell's `D` may hasten. Nothing is drawn
    /// or kept, so for a row read again once answers are drawn it counts as
    /// [`Reads::No`] ([`Unread`]). The cost is a frame's changed lines that
    /// meet those still to be named, and none once a path is owed.
    Pending,
    Yes,
}

/// **What a batch is owed for** ([`Paths::owes`]), the stronger last: a
/// row that changed, which the plain pace answers; or a printed path with
/// no answer, or one on a row still to be named, which that row's naming
/// makes readable -- and drawable, where its answer is kept -- and which
/// the window's shell's `D` may hasten (`probe::Pace::owe`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Owes {
    Rows,
    Path,
}

/// **What the last collection met on rows still to be named**, until
/// their naming: paths whose answers are kept, which the naming draws with
/// no batch after it; or a path with none, for which the naming's answer
/// owes the batch that asks about it ([`Paths::set_pending`]). Over the
/// lines that held them, joined.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Awaiting {
    #[default]
    Nothing,
    Cached(LineSpan),
    Unanswered(LineSpan),
}

impl Awaiting {
    /// With a path met on `span`, its kept `answer` if any.
    fn with(self, answer: Option<State>, span: LineSpan) -> Awaiting {
        let kept = matches!(answer, Some(State::Is(_)));
        match self {
            Awaiting::Nothing if kept => Awaiting::Cached(span),
            Awaiting::Nothing => Awaiting::Unanswered(span),
            Awaiting::Cached(s) if kept => Awaiting::Cached(s.joined(span)),
            Awaiting::Cached(s) | Awaiting::Unanswered(s) => Awaiting::Unanswered(s.joined(span)),
        }
    }
}

/// Whether a scan read a row while printed paths were not read
/// ([`Paths::changed`]), since the last collection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Unread {
    #[default]
    No,
    Yes,
}

/// How many paths a read looked at, for the scan's count of candidates.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Looked(pub usize);

/// What a result batch changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Moved {
    No,
    Yes,
}

/// Whether the scan has met a path it has no answer for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Owed {
    #[default]
    No,
    Yes,
}

/// **Which of the terminal's two surfaces a line is on**: the primary, or
/// the alternate screen as it stood between two of its clears. Each keeps
/// its own count of lines and its own facts ([`Paths`]), so a line of one
/// is never looked up, or filed, as a line of the other; and the alternate
/// screen's count starts again at each of its clears, so a span of an
/// earlier epoch names text that is gone, and is filed nowhere.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    Primary,
    Alternate(AltEpoch),
}

/// **How many times the core has cleared the alternate screen**
/// (`ReadRecord::cleared`), from this window's start.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AltEpoch(u64);

impl AltEpoch {
    fn next(self) -> AltEpoch {
        AltEpoch(self.0.wrapping_add(1))
    }
}

/// **A line on one surface**: the cursor's when a read began or ended, or
/// when a batch was sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineOn {
    pub surface: Surface,
    pub line: AbsLine,
}

/// **Where the core's line numbers fall on the app's for one batch**
/// (`ReadRecord`'s lines, `keep::Counts`): on the primary, the core's line
/// at its window's top as the batch began and the count that numbered it,
/// none where no count places them -- a numbering that clamped the count at
/// the session's first line; each part's lines are placed against the top
/// it ended with ([`CoreLines::part`]). On the alternate screen the count
/// is the core's own number for its top line, so a line is its number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoreLines {
    primary: Option<(baud_domain::LineIdx, Scrolled)>,
    numbers: Numbers,
    rows: Rows,
    alternate: Scrolled,
}

/// **Whether a batch's numbering kept the primary's count on the core's
/// numbers**, or clamped it at the session's first line as the window was
/// taken back past it (`keep::Numbered::PastStart`): every number from the
/// batch's base on then names other text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Numbers {
    Followed,
    Clamped,
}

impl CoreLines {
    /// The primary's window top as the batch began: `core`, the core's
    /// line, numbered here by `scrolled`.
    pub fn of(primary: Option<(baud_domain::LineIdx, Scrolled)>) -> CoreLines {
        CoreLines {
            primary,
            numbers: Numbers::Followed,
            rows: Rows::ZERO,
            alternate: Scrolled::START,
        }
    }

    /// The batch's lines on a window of `rows`, the alternate screen's
    /// numbered by `alternate`, its numbering `numbers`.
    pub fn on(self, rows: Rows, alternate: Scrolled, numbers: Numbers) -> CoreLines {
        CoreLines {
            rows,
            alternate,
            numbers,
            ..self
        }
    }

    /// Whether the batch's numbering kept the primary's count.
    pub fn numbers(&self) -> Numbers {
        self.numbers
    }

    /// **The last line of each surface's window the batch's numbers
    /// reached**: on the primary, the window's last at the batch's base or
    /// at the highest top a part ended with, `tops`, none where nothing
    /// placed the base; on the alternate screen, its window's last.
    pub fn lasts(
        &self,
        tops: impl Iterator<Item = baud_domain::LineIdx>,
    ) -> (Option<AbsLine>, AbsLine) {
        let last = |s: Scrolled| {
            s.line(
                Rows::ZERO,
                baud_domain::Row::from_index(self.rows.index().saturating_sub(1)),
            )
        };
        let primary = self.primary.map(|(base, count)| {
            let up = tops.filter_map(|t| t.since(base)).max();
            last(up.map_or(count, |d| count.advanced(d)))
        });
        (primary, last(self.alternate))
    }

    /// **One part's lines**, its window top `top` as it ended
    /// (`reads::Taken`), numbered here as far from the batch's count as
    /// the core's top moved from the batch's: none for a part with no top,
    /// or one whose top the count cannot number, before the session's
    /// first line.
    pub fn part(&self, top: Option<baud_domain::LineIdx>) -> PartLines {
        let placed = self.primary.filter(|_| self.numbers == Numbers::Followed);
        let primary = placed.zip(top).and_then(|((base, count), top)| {
            let here = match top.since(base) {
                Some(down) => count.advanced(down),
                None => count.retreated(base.since(top)?)?,
            };
            Some((top, here))
        });
        PartLines { primary }
    }
}

/// What a resize answers, and whether its lines are followed.
type Answer = (baud_core::Resized, baud_core::Following);

/// **The lines each re-cut of the primary moved, as the core told them**
/// (`Term::resize_with`, `LineRecut`), kept by a driver and reused: each
/// resize's map sorted by the lines the text held before, with whether the
/// resize told every line it re-cut, and the maps' buffers kept between
/// batches. A window resize takes one ([`Recuts::last`]); a page-width
/// request among a read's parts seals the resizes it made as that part's
/// ([`Recuts::seal`]), and the drain takes each part's in the parts' order
/// ([`Recuts::sealed`]), a part that made one naming it by [`RecutAt`].
#[derive(Debug, Default)]
pub struct Recuts {
    sealed: Vec<Vec<Cut>>,
    open: Vec<Cut>,
    spare: Vec<Vec<baud_core::LineRecut>>,
}

/// One resize's map and whether it told every line it re-cut.
#[derive(Debug)]
struct Cut {
    map: Vec<baud_core::LineRecut>,
    following: baud_core::Following,
}

/// **That a part made resizes, sealed as its own** ([`Recuts::seal`]), with
/// the primary's window top before them (`pre`), on the core's numbers
/// before the re-cut: the window's last line then is one the re-cut may
/// renumber. The drain takes the part's resizes as the next sealed ones,
/// in the parts' order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecutAt {
    pre: baud_domain::LineIdx,
}

impl RecutAt {
    /// Resizes a part made, the window's top `pre` before them.
    pub fn before(pre: baud_domain::LineIdx) -> RecutAt {
        RecutAt { pre }
    }

    /// The primary's window top before the re-cut.
    pub fn pre(&self) -> baud_domain::LineIdx {
        self.pre
    }
}

/// Whether a part made any resize ([`Recuts::seal`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sealed {
    No,
    Yes,
}

/// **One resize's re-cut, as told** ([`Recuts::last`]): the lines, in the
/// order of the text they held before, and whether every line it re-cut
/// was told. Not followed is never read as an empty map.
#[derive(Clone, Copy, Debug)]
pub struct Told<'a> {
    pub map: &'a [baud_core::LineRecut],
    pub following: baud_core::Following,
}

impl Cut {
    fn told(&self) -> Told<'_> {
        Told {
            map: &self.map,
            following: self.following,
        }
    }
}

impl Recuts {
    /// No re-cut held: what a batch is drained with where nothing could
    /// have resized it.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub const NONE: Recuts = Recuts {
        sealed: Vec::new(),
        open: Vec::new(),
        spare: Vec::new(),
    };

    /// Every re-cut forgotten, the maps' buffers kept: a batch's start.
    pub fn clear(&mut self) {
        let cuts = self.sealed.drain(..).flatten().chain(self.open.drain(..));
        for mut c in cuts {
            c.map.clear();
            self.spare.push(c.map);
        }
    }

    /// **`term` resized to `ws`, what its re-cut told kept**
    /// (`Term::resize_with`), sorted by the lines the text held before, in
    /// a buffer an earlier resize left: the resize's own answer.
    pub fn resize(&mut self, term: &mut Term<'_>, ws: &baud_domain::WinSize) -> baud_core::Resized {
        self.cut(|told| term.resize_with(ws, told))
    }

    /// [`Recuts::resize`] for the resize that answers a program's page
    /// width (`Term::resize_page_with`): DECSCPP's and a `?3`'s under
    /// `?95` re-cut nothing and tell nothing, any other `?3`'s or a
    /// `RIS`'s re-cuts as a window's does.
    #[cfg(target_os = "macos")]
    pub fn resize_page(
        &mut self,
        term: &mut Term<'_>,
        ws: &baud_domain::WinSize,
    ) -> baud_core::Resized {
        self.cut(|told| term.resize_page_with(ws, told))
    }

    /// One resize, `resize`, with what it told kept.
    fn cut<F>(&mut self, resize: F) -> baud_core::Resized
    where
        F: FnOnce(&mut dyn FnMut(baud_core::LineRecut)) -> Answer,
    {
        let mut map = self.spare.pop().unwrap_or_default();
        let (resized, following) = resize(&mut |r| map.push(r));
        // Told in the copy's order, newest first where it narrows.
        map.sort_unstable_by_key(|r| r.old.first());
        self.open.push(Cut { map, following });
        resized
    }

    /// The last resize made and not sealed, as told: a window resize's.
    pub fn last(&self) -> Option<Told<'_>> {
        self.open.last().map(Cut::told)
    }

    /// **The resizes made since the last seal, sealed as one part's**:
    /// whether there were any.
    pub fn seal(&mut self) -> Sealed {
        if self.open.is_empty() {
            return Sealed::No;
        }
        self.sealed.push(core::mem::take(&mut self.open));
        Sealed::Yes
    }

    /// **Each sealed part's resizes, as told, in the parts' order**: the
    /// first a drain takes for the first part that names one
    /// ([`RecutAt`]), and so on.
    pub fn sealed(&self) -> impl Iterator<Item = impl Iterator<Item = Told<'_>> + '_> + '_ {
        self.sealed.iter().map(|part| part.iter().map(Cut::told))
    }
}

/// **Where one part's lines fall on the app's numbers**
/// ([`CoreLines::part`]): the core's line at the primary's top as the part
/// ended and the count that numbers it here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PartLines {
    primary: Option<(baud_domain::LineIdx, Scrolled)>,
}

impl PartLines {
    /// **The line here of the core's `line` of `screen`**: none on the
    /// primary where the part could not be placed, or for a line before the
    /// session's first.
    pub fn line(&self, screen: Screen, line: baud_domain::LineIdx) -> Option<AbsLine> {
        match screen {
            Screen::Normal => {
                let (core, here) = self.primary?;
                let at = match line.since(core) {
                    Some(down) => here.advanced(down),
                    None => here.retreated(core.since(line)?)?,
                };
                Some(at.top(Rows::ZERO))
            }
            Screen::Alternate => line.since(baud_domain::LineIdx::FIRST).map(AbsLine::at),
        }
    }

    /// **The core's line of `screen` that is `line` here**: [`PartLines::line`]
    /// undone, none where it places nothing.
    fn core(&self, screen: Screen, line: AbsLine) -> Option<baud_domain::LineIdx> {
        let (core, here) = match screen {
            Screen::Normal => self.primary?,
            Screen::Alternate => (baud_domain::LineIdx::FIRST, Scrolled::START),
        };
        match here.surface_line(line, Rows::of_count(usize::MAX))? {
            baud_core::SurfaceLine::At(r) => Some(core.plus(Rows::of_count(r.index()))),
            baud_core::SurfaceLine::Above(n) => core.minus(n),
        }
    }
}

/// **A run of lines a read erased whole, here** (`ReadRecord::erased`):
/// every cell blanked, so what was filed over them was about text that is
/// gone, and only what is filed after decides them
/// ([`Paths::forget_erased`]). Made only by [`Erased::of`], from the
/// core's record of the read and never from the cells.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Erased(LineSpan);

/// **What a part's record says it erased whole on one screen, placed
/// here** ([`Erased::of`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ErasedLines {
    /// More than the record holds: no line of the part is forgotten.
    Overflowed,
    /// The runs placed, and how many the part's lines could not place,
    /// which keep their facts.
    Placed(Vec<Erased>, u64),
}

impl Erased {
    /// **The runs of `screen` `rec` says its read erased whole, on
    /// `lines`' numbers**, on the surface `paths` says that screen is now.
    /// A run either end of which `lines` cannot place is left out and
    /// counted, failing closed: its lines keep what was filed over them.
    pub fn of(
        rec: &baud_core::ReadRecord,
        lines: &PartLines,
        paths: &Paths,
        screen: Screen,
    ) -> ErasedLines {
        let Some(spans) = rec.erased(screen) else {
            return ErasedLines::Overflowed;
        };
        let surface = paths.surface(screen);
        let (mut placed, mut unplaced) = (Vec::new(), 0);
        for s in spans {
            match (lines.line(screen, s.first()), lines.line(screen, s.last())) {
                (Some(a), Some(b)) => placed.push(Erased(LineSpan::on(surface, a, b))),
                _ => unplaced += 1,
            }
        }
        ErasedLines::Placed(placed, unplaced)
    }

    /// The lines.
    pub fn span(self) -> LineSpan {
        self.0
    }
}

/// **The lines one program wrote, first to last**, on one surface's
/// absolute line numbers, so history scrolling and trimming leave them
/// where they were: what a read was, from the line the cursor was on as it
/// began to the line it ended on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineSpan {
    surface: Surface,
    first: AbsLine,
    last: AbsLine,
}

/// Whether `b` holds the spans `a` does, in whatever order.
fn same_spans(a: &[LineSpan], b: impl Iterator<Item = LineSpan> + Clone) -> bool {
    b.clone().all(|s| a.contains(&s)) && a.iter().all(|p| b.clone().any(|s| s == *p))
}

impl LineSpan {
    /// From `first` to `last` on `surface`, whichever way round they come.
    pub fn on(surface: Surface, first: AbsLine, last: AbsLine) -> LineSpan {
        LineSpan {
            surface,
            first: first.min(last),
            last: first.max(last),
        }
    }

    /// From `first` to `last` on the primary: the surface most tests file.
    #[cfg(test)]
    pub fn of(first: AbsLine, last: AbsLine) -> LineSpan {
        LineSpan::on(Surface::Primary, first, last)
    }

    /// The lines both hold, on one surface; none for none.
    pub fn meet(self, other: LineSpan) -> Option<LineSpan> {
        let (first, last) = (self.first.max(other.first), self.last.min(other.last));
        (self.surface == other.surface && first <= last).then_some(LineSpan {
            surface: self.surface,
            first,
            last,
        })
    }

    /// The span and `other` as one, the lines between included; `other`
    /// alone if it is on another surface, which no span joins.
    pub fn joined(self, other: LineSpan) -> LineSpan {
        match self.surface == other.surface {
            true => LineSpan {
                surface: self.surface,
                first: self.first.min(other.first),
                last: self.last.max(other.last),
            },
            false => other,
        }
    }

    /// The surface.
    pub fn surface(self) -> Surface {
        self.surface
    }

    /// The last line.
    pub fn last(self) -> AbsLine {
        self.last
    }

    /// **The span less `line`**: the lines above it and the lines below it,
    /// each where there are any; the span whole where `line` is not in it.
    pub fn without(self, line: AbsLine) -> impl Iterator<Item = LineSpan> {
        let on = |a, b| LineSpan::on(self.surface, a, b);
        let (above, below) = match self.holds(line) {
            false => (Some(self), None),
            true => (
                line.above()
                    .filter(|&a| a >= self.first)
                    .map(|a| on(self.first, a)),
                (line < self.last).then(|| on(line.below(), self.last)),
            ),
        };
        above.into_iter().chain(below)
    }

    /// **The span less `other`'s lines**: those above `other` and those
    /// below it, each where there are any; the span whole on another
    /// surface.
    pub fn outside(self, other: LineSpan) -> impl Iterator<Item = LineSpan> {
        let on = |a, b| LineSpan::on(self.surface, a, b);
        let (above, below) = match self.surface == other.surface {
            false => (Some(self), None),
            true => (
                other
                    .first
                    .above()
                    .filter(|&a| a >= self.first)
                    .map(|a| on(self.first, a.min(self.last))),
                (other.last < self.last).then(|| on(other.last.below().max(self.first), self.last)),
            ),
        };
        above.into_iter().chain(below)
    }

    /// **The span less the lines above `line`** on `surface`: none where it
    /// lies wholly above; the span whole on another surface.
    pub fn below_or_at(self, surface: Surface, line: AbsLine) -> Option<LineSpan> {
        match self.surface == surface {
            false => Some(self),
            true => (self.last >= line).then(|| LineSpan {
                first: self.first.max(line),
                ..self
            }),
        }
    }

    /// Whether `line`, a line of this span's surface, is in it.
    pub(crate) fn holds(self, line: AbsLine) -> bool {
        self.first <= line && line <= self.last
    }

    /// Whether `other` overlaps this span or begins on the line after it, or
    /// the other way round, on the same surface: two that make one run of
    /// lines.
    pub(crate) fn touches(self, other: LineSpan) -> bool {
        self.surface == other.surface
            && other.first <= self.last.below()
            && self.first <= other.last.below()
    }
}

/// **Where the program that printed a line runs**, as the worker named it
/// when the line was written: what an `OSC 8` `file://` link is read by,
/// since a link names a file only on the machine of the program that
/// printed it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// Written while a program on this machine was in front.
    Local,
    /// Written while `ssh` or its kin, or a program baud could not name,
    /// was: not known to be on this machine.
    Remote,
    /// Not named yet: written since the worker's last answer, or by a read
    /// no group was asked about.
    Unknown,
}

impl Origin {
    /// **The more cautious of two**: remote over not known over local.
    fn or(self, other: Origin) -> Origin {
        match (self, other) {
            (Origin::Remote, _) | (_, Origin::Remote) => Origin::Remote,
            (Origin::Unknown, _) | (_, Origin::Unknown) => Origin::Unknown,
            (Origin::Local, Origin::Local) => Origin::Local,
        }
    }
}

/// **Where the program that printed a link ran, judged once for every way
/// the link is shown or used**: its look at rest, under the pointer, the
/// hand pointer, the preview and its click all take this one value
/// ([`crate::link::admit_link`]). Made only by [`Paths::link_origin`] and
/// [`Paths::bare_origin`], so no caller hands a link an origin of its own.
/// With it, the ssh session that printed the link's rows, which a remote
/// program's `file://` link opens through ([`PathsAt::session`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkOrigin(Origin, Printer);

impl LinkOrigin {
    /// The origin judged.
    pub fn origin(self) -> Origin {
        self.0
    }

    /// **Not known**: what a link no row vouches for, or a gesture over
    /// something no longer a link, is taken as.
    pub fn unknown() -> LinkOrigin {
        LinkOrigin(Origin::Unknown, Printer::None)
    }

    /// An origin a test hands a link, printed under no session.
    #[cfg(test)]
    pub fn given(o: Origin) -> LinkOrigin {
        LinkOrigin(o, Printer::None)
    }
}

/// **The most ssh sessions [`Paths`] tells apart** in one window: past
/// them, rows a new one prints are filed as printed by several, and a
/// `file://` link there opens through none.
const SESSIONS: usize = 16;

/// **A session's place in [`Paths`]'s table of the ssh sessions that
/// printed rows**: given once, never to another session.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct SessionAt(u8);

/// **Which ssh session printed a run of rows**, as the worker read it for
/// the group that wrote them ([`Paths::written_by`]): the session a remote
/// program's `file://` link on them opens through ([`PathsAt::session`]),
/// whatever ssh is in front at the click.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Printer {
    /// No session was filed over them.
    #[default]
    None,
    /// One session.
    One(SessionAt),
    /// A program on this machine, in no session: filed with every local
    /// writer ([`Fact::added_to`]), so a row a session's read covered and
    /// a local program wrote after is printed by two.
    Here,
    /// More than one printer, this machine one of them, or a session past
    /// [`SESSIONS`]: none to open through.
    Several,
}

impl Printer {
    /// **`other` filed over rows that hold `self`**: a first printer
    /// taken, the same one kept, another making several.
    fn with(self, other: Printer) -> Printer {
        match (self, other) {
            (Printer::None, p) | (p, Printer::None) => p,
            (a, b) if a == b => a,
            _ => Printer::Several,
        }
    }

    /// **What two runs of rows read as printed by, together**: one session
    /// only where both hold that one; none where either holds none.
    fn both(self, other: Printer) -> Printer {
        match (self, other) {
            (a, b) if a == b => a,
            (Printer::None, _) | (_, Printer::None) => Printer::None,
            _ => Printer::Several,
        }
    }
}

/// A link target's bytes, hashed: what a slot of the link table names, to
/// tell a slot given another target from the one it held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct UriHash(u64);

impl UriHash {
    /// FNV-1a over the target's bytes.
    fn of(uri: &[u8]) -> UriHash {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in uri {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        UriHash(h)
    }
}

/// **The `OSC 8` link the pen wrote under**, as read after a part of a read
/// was fed (`Term::open_link`): its slot and its target.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pen {
    id: LinkId,
    uri: UriHash,
}

impl Pen {
    /// The link the pen holds now, if one is open.
    pub fn of(term: &Term<'_>) -> Option<Pen> {
        let id = term.open_link()?;
        let uri = term.link_uri(id)?;
        Some(Pen {
            id,
            uri: UriHash::of(uri),
        })
    }
}

/// **The links the pen held as a part of a read was fed**: at its start and
/// at its end, so the read that opened a link and every read under it are
/// filed against its slot (`probe::Probes::drain_taken`). A link opened and
/// closed inside one part is in neither, and needs not be: its cells are on
/// the screen, and the look and the click judge it over them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Pens {
    at_start: Option<Pen>,
    at_end: Option<Pen>,
}

impl Pens {
    /// The pen at a part's start and at its end.
    pub fn of(at_start: Option<Pen>, at_end: Option<Pen>) -> Pens {
        Pens { at_start, at_end }
    }

    /// Each link held, once.
    pub fn each(self) -> impl Iterator<Item = Pen> {
        let end = self.at_end.filter(|e| Some(*e) != self.at_start);
        self.at_start.into_iter().chain(end)
    }
}

/// **Every read that opened a link or wrote under it**, for one slot of the
/// link table: the target it held then and the lines those reads wrote,
/// joined, on each surface they wrote ([`Paths::slot_wrote`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct SlotRecord {
    uri: UriHash,
    primary: Option<LineSpan>,
    alternate: Option<LineSpan>,
}

impl SlotRecord {
    /// The lines on `span`'s surface, joined with `span`.
    fn joined(self, span: LineSpan) -> SlotRecord {
        let join = |was: Option<LineSpan>| Some(was.map_or(span, |w| w.joined(span)));
        match span.surface {
            Surface::Primary => SlotRecord {
                primary: join(self.primary),
                ..self
            },
            Surface::Alternate(_) => SlotRecord {
                alternate: join(self.alternate),
                ..self
            },
        }
    }
}

/// What the program in front is, as the worker last named it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Front {
    /// None named yet, or one on this machine.
    #[default]
    Local,
    /// A terminal multiplexer: `./` is a pane's, which baud cannot see.
    Multiplexer,
    /// `ssh` and its kin, or a program baud could not name: not known to be
    /// on this machine.
    Elsewhere,
}

/// **Who the worker found wrote a span**: a program it named, or none it
/// could name -- one gone before it was asked about, or rows written past
/// the groups a batch keeps, which no one looked at. Not named is neither
/// local nor remote: its rows fail closed and touch no other row, unless a
/// command the shell named here, with its token, wrote them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seen {
    Named(Front),
    NotNamed,
}

/// **What wrote a row**, from the facts filed over it ([`Facts::provenance`]),
/// in the order one outweighs another: a row is local only if everything
/// filed over it is, any remote writer makes it remote, and a writer not
/// named makes it not known unless a command the shell named here, with its
/// token, wrote it. What decides whether a path printed there is read
/// ([`Paths::readable_on`]) and where a `file://` link there was printed
/// ([`Paths::origin`]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Provenance {
    /// A program named on this machine, or a writer not named that a
    /// command the shell named here wrote under.
    Local,
    /// A multiplexer: its panes are this machine's, and `./` is a pane's.
    Multiplexed,
    /// A writer not named: gone before it was asked about, or past the
    /// groups a batch keeps.
    NotNamed,
    /// A program named not on this machine, by the worker, the job watch or
    /// the shell's marks.
    Remote,
}

/// **The weightiest writer named over a run of rows**, in that order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Writer {
    Local,
    Multiplexed,
    Remote,
}

/// Whether a writer not named wrote a run of rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Unnamed {
    #[default]
    No,
    Yes,
}

/// Whether a read nothing names -- no group sampled for it, no marks of the
/// shell's -- wrote a run of rows ([`Paths::written_unnamed`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Nameless {
    #[default]
    No,
    Yes,
}

/// Whether a command the shell named here, with its token, wrote a run of
/// rows (`crate::ran::Ran::Here`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Here {
    #[default]
    No,
    Yes,
}

/// Whether text a read moved onto a run of rows came from a line no fact
/// vouched for ([`Paths::carried`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Carried {
    #[default]
    Vouched,
    Unvouched,
}

/// **What was filed over a run of rows**, each kind of fact kept apart, so
/// the order the facts arrive in -- a worker's verdict usually after the
/// read it is about -- changes nothing ([`Facts::provenance`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Facts {
    writer: Option<Writer>,
    unnamed: Unnamed,
    nameless: Nameless,
    here: Here,
    carried: Carried,
    printer: Printer,
}

/// One fact filed over a span of rows ([`Paths::file`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fact {
    /// A writer named: the worker's verdict on a group, the front it left,
    /// the job watch's or the shell's marks' remote command.
    Writer(Writer),
    /// A writer the worker could not name.
    NotNamed,
    /// A read nothing names wrote the rows: no writer to name, so no
    /// command's lift reaches it.
    Nameless,
    /// A command the shell named here wrote the rows.
    Here,
    /// A read moved text onto the rows from a line no fact vouched for.
    Unvouched,
    /// The ssh session the rows' writer runs in.
    Printer(Printer),
}

impl Fact {
    fn of(seen: Seen) -> Fact {
        match seen {
            Seen::Named(Front::Local) => Fact::Writer(Writer::Local),
            Seen::Named(Front::Multiplexer) => Fact::Writer(Writer::Multiplexed),
            Seen::Named(Front::Elsewhere) => Fact::Writer(Writer::Remote),
            Seen::NotNamed => Fact::NotNamed,
        }
    }

    /// `facts` with this one added: the weightier writer kept, a flag set.
    fn added_to(self, facts: Facts) -> Facts {
        match self {
            Fact::Writer(w) => Facts {
                writer: Some(facts.writer.map_or(w, |was| was.max(w))),
                printer: match w {
                    Writer::Local => facts.printer.with(Printer::Here),
                    Writer::Multiplexed | Writer::Remote => facts.printer,
                },
                ..facts
            },
            Fact::NotNamed => Facts {
                unnamed: Unnamed::Yes,
                ..facts
            },
            Fact::Nameless => Facts {
                nameless: Nameless::Yes,
                ..facts
            },
            Fact::Here => Facts {
                here: Here::Yes,
                ..facts
            },
            Fact::Unvouched => Facts {
                carried: Carried::Unvouched,
                ..facts
            },
            Fact::Printer(p) => Facts {
                printer: facts.printer.with(p),
                ..facts
            },
        }
    }

    /// **What to file over a line whose text came from lines whose most
    /// cautious writer was `worst`** ([`Paths::carried`]): that writer, or,
    /// for lines no writer was named for, a fact no other lifts.
    fn carried(worst: Provenance) -> Fact {
        match worst {
            Provenance::Local => Fact::Writer(Writer::Local),
            Provenance::Multiplexed => Fact::Writer(Writer::Multiplexed),
            Provenance::NotNamed => Fact::Unvouched,
            Provenance::Remote => Fact::Writer(Writer::Remote),
        }
    }
}

impl Facts {
    /// **What wrote the rows, by every fact filed over them**: the
    /// weightiest of the writer named and a writer not named, the second
    /// lifted to local where a command the shell named here wrote them --
    /// a lift of that fact alone, never of a remote writer or a
    /// multiplexer, and nothing where no writer was named at all -- nor of
    /// a read nothing names, nor of text a read moved from a line no fact
    /// vouched for. None for no fact,
    /// which no reader takes as local.
    fn provenance(self) -> Option<Provenance> {
        let named = self.writer.map(|w| match w {
            Writer::Local => Provenance::Local,
            Writer::Multiplexed => Provenance::Multiplexed,
            Writer::Remote => Provenance::Remote,
        });
        let unnamed = match (self.unnamed, self.here) {
            (Unnamed::No, _) => None,
            (Unnamed::Yes, Here::Yes) => Some(Provenance::Local),
            (Unnamed::Yes, Here::No) => Some(Provenance::NotNamed),
        };
        let nameless = match self.nameless {
            Nameless::No => None,
            Nameless::Yes => Some(Provenance::NotNamed),
        };
        let carried = match self.carried {
            Carried::Vouched => None,
            Carried::Unvouched => Some(Provenance::NotNamed),
        };
        named.max(unnamed).max(nameless).max(carried)
    }
}

/// **Whether the folder the core holds may be read against**: the last read
/// that carried a folder report decides (`probe::Probes::read_taken`). One
/// written under a command the shell named not local is that program's,
/// which can name this machine as well as the shell can, and leaves the
/// folder read against as it was until a report of the shell's comes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Report {
    #[default]
    Taken,
    Refused,
}

/// Whether path marks are on, and if not, why.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Marks {
    /// `[link] paths = "off"`, the default until a driver turns them on.
    #[default]
    Off,
    On,
    /// The worker stopped answering: off for this window (`probe.rs`).
    Closed,
}

/// What a kept answer is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    /// In a batch, not answered yet.
    Asked,
    Is(Probe),
    /// An answer drawn as it is until the next batch asks again: a struck
    /// link's, when a job ended, the window came to the front or the
    /// pointer came over it.
    Stale(Probe),
}

impl State {
    /// The answer drawn, if any.
    fn probe(self) -> Option<Probe> {
        match self {
            State::Is(p) | State::Stale(p) => Some(p),
            State::Asked => None,
        }
    }
}

/// **The folder of the program in front**, as the worker read it for the
/// foreground group (`probe.rs`): what a relative path on the alternate
/// screen is read against, since a full-screen program reports no folder of
/// its own. An absolute path, kept only while the alternate screen is up.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProgramDir(pub Vec<u8>);

/// **Which folder a relative path is read against**: the one the shell
/// reported, on the normal screen, or the program's in front, on the
/// alternate screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Resolve {
    Reported,
    Program,
}

impl Resolve {
    fn on(screen: Screen) -> Resolve {
        match screen {
            Screen::Normal => Resolve::Reported,
            Screen::Alternate => Resolve::Program,
        }
    }
}

/// The reported folder, as baud keeps it: the last report's bytes, and the
/// line where it took effect.
#[derive(Default, Debug)]
struct Dir {
    /// `None`: none reported. The host's bytes (empty for none or
    /// `localhost`) and the path, as reported.
    kept: Option<(Option<Vec<u8>>, Vec<u8>)>,
    /// The path, when the report names this machine by name.
    local: Option<Vec<u8>>,
    /// The first line a `./` may be read against it: where the cursor was at
    /// the first scan that saw the report.
    from: Option<AbsLine>,
}

/// A collection in progress: the answers the screen still shows, and what
/// to ask.
#[derive(Default, Debug)]
struct Collect {
    kept: HashMap<Vec<u8>, State>,
    /// The `OSC 8` `file://` links' answers the screen still shows.
    marked: HashMap<Vec<u8>, State>,
    queue: Vec<Candidate>,
    /// Single names to ask, at most [`NAMES_MAX`], given the slots `queue`
    /// leaves at the batch's end.
    names: Vec<Candidate>,
    /// The paths met in a form that is not a single name, at least once.
    prefixed: HashSet<Vec<u8>>,
    over: Over,
}

/// **Printed paths: the scan's reader, the answers, and what decides which
/// rows are read.** A driver keeps one a window, the worker's answers merged
/// into it ([`Paths::merge`]); the frame only reads it.
#[derive(Debug, Default)]
pub struct Paths {
    marks: Marks,
    forms: Forms,
    home: Option<Vec<u8>>,
    names: crate::link::MachineNames,
    dir: Dir,
    /// Whether the last folder report may be read against ([`Report`]).
    report: Report,
    /// The folder of the program in front, while the alternate screen is up.
    program_dir: Option<ProgramDir>,
    geometry: Option<(Cols, Rows)>,
    known: HashMap<Vec<u8>, State>,
    /// Which of `known`'s paths the screen holds in a form that is not a
    /// single name: under `names = "hover"`, the only ones drawn at rest.
    prefixed: HashSet<Vec<u8>>,
    /// `OSC 8` `file://` links' answers, kept apart from printed paths' so
    /// one found does not make a frame read rows for printed paths.
    marked: HashMap<Vec<u8>, State>,
    /// `[link] missing`: whether anything is struck.
    missing: Turned,
    /// `[link] join_rows`: whether a row the writer ended is read on into
    /// the next.
    join: Turned,
    /// What the worker last found for each program it was asked about.
    programs: Vec<(Program, Handler)>,
    generation: Generation,
    /// What a batch is owed for, if one is.
    owing: Option<Owes>,
    collect: Option<Collect>,
    reads: Reads,
    /// What the last collection met on rows still to be named.
    awaiting: Awaiting,
    /// Whether a scan read a row while printed paths were not read, since
    /// the last collection read every row: a row it kept no run for.
    unread: Unread,
    /// What is in front, and the line it was named at when it is not
    /// local: rows from there are its while it stays.
    front: Front,
    front_from: Option<LineOn>,
    /// **What was filed over each surface's lines** ([`Store`]): the
    /// primary's, and the alternate screen's since its last clear, the
    /// epoch it is in. A buffer the filing reuses, and the steps every
    /// lookup has taken ([`Paths::facts_at`]).
    primary: Store,
    alternate: Store,
    alt_epoch: AltEpoch,
    pieces: Vec<(LineSpan, Facts)>,
    looked: core::cell::Cell<Tally>,
    /// Moved by every span named, which may join or part two rows
    /// ([`PathsAt::origins`]).
    origins: Generation,
    /// **The lines a read's own filing moved the facts of**, as one span
    /// until the next scan takes it ([`Reader::refiled`]): a fact moves
    /// what a link on a row reads, with no change to the row's cells.
    refiled: Option<LineSpan>,
    /// **The lines whose writer is still to be named, where their text is
    /// now** (`probe::Probes`'s: kept to send, and sent and not yet
    /// answered): a gap among them carries nothing when a read moves it
    /// ([`Paths::worst_over`]), since its span goes with the text.
    pending: Vec<LineSpan>,
    /// **The lines of those an answer for another writer was filed over**
    /// (`probe::Probes`'s holds): none reads as this machine's, whatever is
    /// filed over it ([`Paths::wrote`]), until its own writer is named. A
    /// span carried through a move it did not make exactly -- the hull of a
    /// move that took part of it, a band -- covers lines another writer's
    /// text may hold, so a local answer filed over it stands on them only
    /// once their own writer's does.
    held: Vec<LineSpan>,
    key: Vec<u8>,
    tokens: Vec<Token>,
    /// The paths the last read found runs for, each run's [`Key::Path`] a
    /// range of it.
    read_keys: Vec<u8>,
    /// Your `[link.*]` rules, the forms among them, which a read of the link
    /// under the pointer goes by ([`PathsAt::rules`]).
    rules: Rules,
    /// **Each slot of the `OSC 8` link table's record** ([`SlotRecord`]),
    /// by its index, and the table's epoch it was last checked at: a slot
    /// given another target since forgets its record
    /// ([`Paths::observe`]).
    slots: Vec<Option<SlotRecord>>,
    slots_at: Option<LinkEpoch>,
    /// **The last re-cut's lines, before and after, here** ([`Paths::reflowed`]),
    /// in the order of the text before, and what each takes on its new
    /// lines: kept until the next, so the driver's spans are carried by the
    /// same lines ([`Paths::recut_span`]), and reused.
    recut: Vec<(LineSpan, LineSpan, Option<Fact>)>,
    /// **The ssh session in front**, as the worker last read it
    /// ([`Paths::set_session`]): what the rows its group writes while it
    /// stays in front are filed as printed by, when no batch names them
    /// (`probe::Probes`).
    session: Option<std::sync::Arc<crate::ssh::Session>>,
    /// **Each ssh session that printed rows**, at most [`SESSIONS`], by the
    /// place a [`Printer`] names: a remote program's `file://` link opens
    /// through the one its rows were printed under, for its click, its
    /// preview and its look at rest alike.
    sessions: Vec<std::sync::Arc<crate::ssh::Session>>,
}

/// **What was filed over one surface's lines**: each run of lines, sorted,
/// apart, and runs of equal facts that touch merged ([`Paths::file`]) -- a
/// writer not local darkens its own lines and no other -- and the line up
/// to which every line was renumbered in a way no count says
/// ([`Paths::renumbered`]), not known whatever is filed over it after.
#[derive(Debug, Default)]
struct Store {
    runs: Vec<(LineSpan, Facts)>,
    floor: Option<AbsLine>,
}

impl Store {
    /// Whether the renumbering covers `line`.
    fn floored(&self, line: AbsLine) -> bool {
        self.floor.is_some_and(|l| line <= l)
    }

    /// [`Paths::file`] in this store, `pieces` a buffer it reuses.
    fn file(&mut self, fact: Fact, span: LineSpan, pieces: &mut Vec<(LineSpan, Facts)>) -> Moved {
        let on = |a, b| LineSpan::on(span.surface, a, b);
        let from = self.runs.partition_point(|(r, _)| r.last < span.first);
        pieces.clear();
        let mut moved = Moved::No;
        // The next line of `span` no run has covered yet; none once past
        // its end.
        let mut next = Some(span.first);
        let mut to = from;
        while let Some(&(run, facts)) = self.runs.get(to) {
            if run.first > span.last {
                break;
            }
            if let (Some(n), Some(before)) = (next, run.first.above()) {
                if n < run.first {
                    pieces.push((on(n, before), fact.added_to(Facts::default())));
                    moved = Moved::Yes;
                }
            }
            if let Some(left) = span.first.above().filter(|_| run.first < span.first) {
                pieces.push((on(run.first, left), facts));
            }
            let now = fact.added_to(facts);
            if now != facts {
                moved = Moved::Yes;
            }
            pieces.push((on(run.first.max(span.first), run.last.min(span.last)), now));
            if run.last > span.last {
                pieces.push((on(span.last.below(), run.last), facts));
            }
            next = (run.last < span.last).then(|| run.last.below());
            to += 1;
        }
        if let Some(n) = next.filter(|n| *n <= span.last) {
            pieces.push((on(n, span.last), fact.added_to(Facts::default())));
            moved = Moved::Yes;
        }
        let placed = pieces.len();
        self.runs.splice(from..to, pieces.drain(..));
        // Merge runs of equal facts that touch, around what was placed.
        let mut i = from.saturating_sub(1);
        let end = from + placed;
        while i < end && i + 1 < self.runs.len() {
            let (a, fa) = self.runs[i];
            let (b, fb) = self.runs[i + 1];
            if fa == fb && a.last.below() == b.first {
                self.runs[i] = (on(a.first, b.last), fa);
                self.runs.remove(i + 1);
            } else {
                i += 1;
            }
        }
        moved
    }

    /// **The facts over `span` gone** ([`Paths::forget_erased`]): each run
    /// it covers cut to the lines outside it, which keep theirs, and the
    /// floor left where it is -- a line renumbered in a way no count says
    /// stays not known. Whether anything was filed there. The runs are
    /// sorted and apart, so only the first it covers can keep lines above
    /// it and only the last lines below: at most two pieces, put in place
    /// of the runs from an exact two-slot iterator, so nothing is collected
    /// and the runs' buffer grows only where one run is cut in two.
    fn forget_lines(&mut self, span: LineSpan) -> Moved {
        let from = self.runs.partition_point(|(r, _)| r.last < span.first);
        let to = from
            + self.runs[from..]
                .iter()
                .take_while(|(r, _)| r.first <= span.last)
                .count();
        if from == to {
            return Moved::No;
        }
        let (first, above_facts) = self.runs[from];
        let (last, below_facts) = self.runs[to - 1];
        let above = first.outside(span).next().filter(|p| p.first < span.first);
        let below = last.outside(span).last().filter(|p| p.last > span.last);
        let kept = above.map(|p| (p, above_facts)).into_iter();
        self.runs
            .splice(from..to, kept.chain(below.map(|p| (p, below_facts))));
        Moved::Yes
    }
}

impl Paths {
    /// **Paths for a window**, `home` the `HOME` it started with -- an
    /// absolute path or none -- and `forms` what is read.
    pub fn new(home: Option<Vec<u8>>, forms: Forms) -> Paths {
        Paths {
            marks: Marks::Off,
            forms,
            home: home.filter(|h| h.first() == Some(&b'/')),
            ..Paths::default()
        }
    }

    /// `[link] paths`: on, or off with every answer dropped; a window whose
    /// worker stopped answering stays closed.
    pub fn set(&mut self, on: Turned) {
        self.marks = match (self.marks, on) {
            (Marks::Closed, _) => Marks::Closed,
            (_, Turned::On) => Marks::On,
            (_, Turned::Off) => {
                self.drop_all();
                Marks::Off
            }
        };
        self.update_reads();
    }

    /// **Your `[link.*]` rules, as a load read them**: the forms read from
    /// here on, and what a read of the link under the pointer admits with.
    /// The answers move, so such a read is made again, and a batch is owed
    /// for the forms the rows may now hold; the caller reads every row again
    /// (`Lines::forget`).
    pub fn set_rules(&mut self, rules: Rules) {
        self.forms = rules.forms();
        self.rules = rules;
        self.generation = self.generation.next();
        self.changed();
        self.update_reads();
    }

    /// Whether anything is read, asked or drawn.
    pub fn marks(&self) -> Marks {
        self.marks
    }

    /// **The worker stopped answering**: every mark goes, and none comes
    /// again in this window, and no session is in front any more.
    pub fn close(&mut self) {
        self.session = None;
        self.drop_all();
        self.marks = Marks::Closed;
        self.update_reads();
    }

    fn drop_all(&mut self) {
        self.known.clear();
        self.prefixed.clear();
        self.marked.clear();
        // What the worker found for a program goes too: with no worker to
        // ask it again, a kind struck for having no app would stay struck.
        self.programs.clear();
        self.collect = None;
        self.owing = None;
        self.awaiting = Awaiting::Nothing;
        self.generation = self.generation.next();
        self.update_reads();
    }

    /// [`Reads`] again, after the marks, the collection, the answers or the
    /// lines still to be named changed: at a batch or a drain, never a
    /// frame.
    fn update_reads(&mut self) {
        // A single name draws nothing at rest under `names = "hover"`: lit
        // only under the pointer, it needs no row read for it.
        let names = self.forms.names != NameLook::Hover;
        let draws = self
            .known
            .iter()
            .any(|(k, s)| s.probe() == Some(Probe::Exists) && (names || self.prefixed.contains(k)));
        let on = self.marks == Marks::On;
        self.reads = match (
            on && (self.collect.is_some() || draws),
            on && !self.pending.is_empty(),
        ) {
            (true, _) => Reads::Yes,
            (false, true) => Reads::Pending,
            (false, false) => Reads::No,
        };
    }

    /// **A batch is owed for `o`**, or for what it was owed for already if
    /// that is the stronger.
    fn owe(&mut self, o: Owes) {
        self.owing = Some(self.owing.map_or(o, |was| was.max(o)));
    }

    /// **A row of the window changed**: a batch is owed for the rows
    /// ([`Owes::Rows`]), which the next one due answers by reading every row
    /// then. One store a scan.
    fn changed(&mut self) {
        if self.marks == Marks::On {
            self.owe(Owes::Rows);
        }
        if self.reads != Reads::Yes {
            self.unread = Unread::Yes;
        }
    }

    /// **The ssh session in front, as the worker read it in an answer**:
    /// none while no ssh leads the group in front, or for one that is no
    /// session a file can be opened in. No link opens by it: each opens by
    /// the session filed over its own rows ([`Paths::written_by`]), and this
    /// is the one rows its group writes are filed under while it stays in
    /// front ([`Paths::front_session`]).
    pub fn set_session(&mut self, session: Option<std::sync::Arc<crate::ssh::Session>>) {
        self.session = session;
    }

    /// The ssh session in front ([`Paths::set_session`]).
    pub fn front_session(&self) -> Option<std::sync::Arc<crate::ssh::Session>> {
        self.session.clone()
    }

    /// This machine's names, once read: what an `OSC 7` report must name,
    /// and a `file://` link's host to be this machine's.
    pub fn set_names(&mut self, names: crate::link::MachineNames) {
        self.names = names;
        self.dir.local = self.local_of_kept();
        self.generation = self.generation.next();
    }

    /// The names [`Paths::set_names`] gave.
    #[cfg(test)]
    pub fn names(&self) -> &crate::link::MachineNames {
        &self.names
    }

    fn local_of_kept(&self) -> Option<Vec<u8>> {
        let (Some(h), path) = self.dir.kept.as_ref()? else {
            return None;
        };
        let named = self.names.is_this(h);
        (named && path.first() == Some(&b'/')).then(|| path.clone())
    }

    /// The answers' generation: moved by every batch that changed one, the
    /// names, or what is in front, for a read of the link under the pointer
    /// to read again.
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Whether a row of the window changed, or a scan met a path it has no
    /// answer for, since the last batch.
    pub fn owed(&self) -> Owed {
        match self.owing {
            Some(_) => Owed::Yes,
            None => Owed::No,
        }
    }

    /// **What the batch owed is owed for**: none while none is owed.
    pub fn owes(&self) -> Option<Owes> {
        self.owing
    }

    /// **What the worker said is there**, for the strike: none for a path it
    /// was not asked about, or not yet answered.
    #[cfg(test)]
    pub fn result(&self, path: &[u8]) -> Option<Probe> {
        self.known
            .get(path)
            .or_else(|| self.marked.get(path))
            .and_then(|s| s.probe())
    }

    /// **`[link] join_rows`**: off, no read joins a row the writer ended to
    /// the next ([`PathsAt::joins`]). Whether it changed, which reads every
    /// row again.
    pub fn set_join(&mut self, join: Turned) -> Moved {
        if self.join == join {
            return Moved::No;
        }
        self.join = join;
        self.generation = self.generation.next();
        Moved::Yes
    }

    /// **The answers kept for printed paths**: how many, and their keys'
    /// bytes, for the measurement of what names cost (`probe.rs`).
    #[cfg(test)]
    pub fn known_size(&self) -> (usize, usize) {
        (self.known.len(), self.known.keys().map(Vec::len).sum())
    }

    /// **`[link] missing`**: off strikes nothing and asks nothing for it.
    /// Whether it changed, which reads every row again.
    pub fn set_missing(&mut self, missing: Turned) -> Moved {
        if self.missing == missing {
            return Moved::No;
        }
        self.missing = missing;
        self.generation = self.generation.next();
        Moved::Yes
    }

    /// **What the worker found for the programs it was asked about**, each
    /// kept until the next answer: whether a mark moved.
    pub fn set_programs(&mut self, found: Vec<(Program, Handler)>) -> Moved {
        let mut moved = Moved::No;
        for (p, h) in found {
            match self.programs.iter_mut().find(|(q, _)| *q == p) {
                Some((_, kept)) if *kept == h => {}
                Some((_, kept)) => {
                    *kept = h;
                    moved = Moved::Yes;
                }
                None => {
                    self.programs.push((p, h));
                    moved = Moved::Yes;
                }
            }
        }
        if moved == Moved::Yes {
            self.generation = self.generation.next();
        }
        moved
    }

    /// Whether the worker found some program missing: what a job ending
    /// asks about again, since one may have been installed. macOS's driver
    /// sees the job in front fork and exit; Linux's, its foreground group
    /// changing at the title's check.
    pub fn programs_missing(&self) -> bool {
        self.programs.iter().any(|(_, h)| *h == Handler::Missing)
    }

    /// What the worker last found for `program`: unknown until it answers.
    pub fn program(&self, program: &Program) -> Handler {
        self.programs
            .iter()
            .find(|(p, _)| p == program)
            .map_or(Handler::Unknown, |(_, h)| *h)
    }

    /// **How a link `admit` opens by `scheme` is drawn**: struck only while
    /// `missing` is on and the worker found no app for it.
    pub fn scheme_mark(&self, scheme: Scheme) -> Mark {
        self.program_mark(&Program::Scheme(scheme))
    }

    /// **How a link `admit` let through as `target` is drawn**: one a `[link.NAME]`
    /// rule takes and runs a command for by that command's program, else by
    /// its kind ([`Paths::scheme_mark`]).
    /// A rule's copy is never struck: a click copies, whatever opens the kind.
    pub fn target_mark(&self, target: &Target<'_>) -> Mark {
        match self.rule_act(target) {
            Some(crate::rules::Act::Command(argv) | crate::rules::Act::Window(argv)) => {
                self.program_mark(&Program::Command(argv.program().to_owned()))
            }
            Some(crate::rules::Act::Copy) => Mark::Line,
            _ => self.scheme_mark(target.scheme()),
        }
    }

    /// What the `[link.NAME]` rule that let `target` through does, if one did.
    fn rule_act(&self, target: &Target<'_>) -> Option<&crate::rules::Act> {
        target
            .rule()
            .and_then(|ix| self.rules.at(ix))
            .map(|r| r.act())
    }

    /// **How a link a `[link.*]` rule opens with `program` is drawn**: the
    /// hook a `command` rule's `command[0]` reads, struck only while
    /// `missing` is on and the worker found none of that name.
    pub fn program_mark(&self, program: &Program) -> Mark {
        match (self.missing, self.program(program)) {
            (Turned::On, Handler::Missing) => Mark::Struck,
            _ => Mark::Line,
        }
    }

    /// **Ask again about what is struck**, at the next batch, drawing it as
    /// it is until then: a job ended or the window came to the front, and a
    /// file may have come back. No timer asks this.
    pub fn recheck(&mut self) {
        let mut any = false;
        for s in self.known.values_mut().chain(self.marked.values_mut()) {
            if *s == State::Is(Probe::Missing) {
                *s = State::Stale(Probe::Missing);
                any = true;
            }
        }
        if any && self.marks == Marks::On {
            self.owe(Owes::Path);
        }
    }

    /// **Ask again about one path**, at the next batch: the pointer came
    /// over a link struck for it.
    pub fn recheck_path(&mut self, path: &[u8]) {
        for map in [&mut self.known, &mut self.marked] {
            if let Some(s) = map.get_mut(path) {
                if let State::Is(p) = *s {
                    *s = State::Stale(p);
                }
            }
        }
        if self.marks == Marks::On {
            self.owe(Owes::Path);
        }
    }

    /// **The reported folder, before a scan**: a report that differs from the
    /// one kept takes effect at the cursor's line, as does a change of size,
    /// whose reflow moves lines; a screen cleared under the cursor takes the
    /// line down to it. Whether the rows a `./` may be read on moved, which
    /// reads every row again.
    pub fn observe(&mut self, term: &Term<'_>, scrolled: Scrolled) -> Moved {
        self.check_slots(term);
        if self.marks != Marks::On {
            return Moved::No;
        }
        let cursor = scrolled.line(Rows::ZERO, term.cursor().0);
        let mut moved = Moved::No;
        // The program's folder goes with its screen: the next full-screen
        // program's is read for it.
        if term.screen() == Screen::Normal && self.set_program_dir(None) == Moved::Yes {
            moved = Moved::Yes;
        }
        let geometry = term.geometry();
        if self.geometry != Some(geometry) {
            self.geometry = Some(geometry);
            if self.dir.from.is_some() {
                self.dir.from = Some(cursor);
                moved = Moved::Yes;
            }
        }
        let now = term.working_directory();
        let same = match (&self.dir.kept, now) {
            (None, None) => true,
            (Some((h, p)), Some(d)) => {
                let host_same = match (h, d.host) {
                    (None, FileHost::Local) => true,
                    (Some(h), FileHost::Named(n)) => h.as_slice() == n,
                    _ => false,
                };
                host_same && p.as_slice() == d.path
            }
            _ => false,
        };
        if !same && self.report == Report::Taken {
            self.dir.kept = now.map(|d| {
                let host = match d.host {
                    FileHost::Local => None,
                    FileHost::Named(n) => Some(n.to_vec()),
                };
                (host, d.path.to_vec())
            });
            self.dir.local = self.local_of_kept();
            self.dir.from = now.map(|_| cursor);
            return Moved::Yes;
        }
        // `clear`: the cursor above the line, with nothing below it to it.
        if let Some(from) = self.dir.from {
            if cursor < from
                && term.screen() == Screen::Normal
                && blank_between(term, scrolled, cursor, from)
            {
                self.dir.from = Some(cursor);
                moved = Moved::Yes;
            }
        }
        moved
    }

    /// The folder a `./` path on the normal screen is read against now.
    #[cfg(test)]
    pub(crate) fn folder_now(&self) -> Option<&[u8]> {
        self.dir.local.as_deref()
    }

    /// **A read carried a folder report**, which `report` says may be read
    /// against or not ([`Report`]): the next [`Paths::observe`] takes the
    /// core's folder, or leaves the one it has.
    pub fn folder_reported(&mut self, report: Report) {
        self.report = report;
    }

    /// **The folder of the program in front**, as the worker read it, or
    /// none: the rows are read again against it, and a batch is owed so a
    /// screen that does not change is asked about too. One that is not an
    /// absolute path is none.
    pub fn set_program_dir(&mut self, dir: Option<ProgramDir>) -> Moved {
        let dir = dir.filter(|d| d.0.first() == Some(&b'/'));
        if self.program_dir == dir {
            return Moved::No;
        }
        self.program_dir = dir;
        self.generation = self.generation.next();
        self.changed();
        self.update_reads();
        Moved::Yes
    }

    /// **The surface `screen` is now**: the primary, or the alternate
    /// screen in the epoch it is in.
    pub fn surface(&self, screen: Screen) -> Surface {
        match screen {
            Screen::Normal => Surface::Primary,
            Screen::Alternate => Surface::Alternate(self.alt_epoch),
        }
    }

    /// **Line `line` of `screen` as it is now** ([`Paths::surface`]).
    pub fn line_on(&self, screen: Screen, line: AbsLine) -> LineOn {
        LineOn {
            surface: self.surface(screen),
            line,
        }
    }

    /// The store `surface`'s lines are filed in: none for the alternate
    /// screen of an epoch past, whose text a clear took.
    fn store(&self, surface: Surface) -> Option<&Store> {
        match surface {
            Surface::Primary => Some(&self.primary),
            Surface::Alternate(e) => (e == self.alt_epoch).then_some(&self.alternate),
        }
    }

    fn store_mut(&mut self, surface: Surface) -> Option<&mut Store> {
        match surface {
            Surface::Primary => Some(&mut self.primary),
            Surface::Alternate(e) => (e == self.alt_epoch).then_some(&mut self.alternate),
        }
    }

    /// **The core cleared the alternate screen** (`Cleared::Alternate`:
    /// `?1049h` from either screen, `?1047l`, `RIS`, `DECCOLM` while it is
    /// up): its facts go with its text, and its next lines, which the count
    /// numbers from its start again, are a new epoch's -- a span of the
    /// last, a worker's answer still on its way included, is filed nowhere.
    pub fn alternate_cleared(&mut self) {
        self.alternate = Store::default();
        self.alt_epoch = self.alt_epoch.next();
        self.origins = self.origins.next();
        self.generation = self.generation.next();
        self.changed();
    }

    /// **The lines `erased` names were erased whole**, as the core recorded
    /// the read that did it: a fact is about the text it was filed with,
    /// so every one filed over them ends with that text, and only those
    /// filed after decide them -- a remote writer's still makes them
    /// remote, and one not named still leaves them not known. A line
    /// written over in part is never named here, and keeps what was filed.
    /// The floor stays. Every row is read again, and a link on them looked
    /// at again ([`Reader::refiled`]).
    pub fn forget_erased(&mut self, erased: Erased) {
        let span = erased.0;
        let Some(store) = self.store_mut(span.surface) else {
            return;
        };
        if store.forget_lines(span) == Moved::Yes {
            self.origins = self.origins.next();
            self.generation = self.generation.next();
            self.refiled = Some(self.refiled.map_or(span, |r| r.joined(span)));
            self.changed();
        }
    }

    /// **What the worker named in front**, at the line the cursor was on
    /// when it was asked: rows from there are its while it stays in front,
    /// and rows to there were filed by the spans it and the programs before
    /// it wrote. A program not on this machine, or a multiplexer, leaves its
    /// rows from that line on behind it when another comes in front.
    pub fn set_front(&mut self, front: Front, through: LineOn) {
        if self.front != front {
            self.leave_front(through);
            self.front_from = match front {
                Front::Local => None,
                Front::Elsewhere | Front::Multiplexer => Some(through),
            };
            self.front = front;
            self.generation = self.generation.next();
        }
    }

    /// **The program in front leaves**: its rows from where it was named to
    /// `through` are filed under it. Named on one surface and leaving on the
    /// other, it may have written either from its line on, so the first
    /// surface's lines from there and the second's to `through` are filed
    /// under it, failing closed.
    fn leave_front(&mut self, through: LineOn) {
        let writer = match self.front {
            Front::Local => return,
            Front::Elsewhere => Writer::Remote,
            Front::Multiplexer => Writer::Multiplexed,
        };
        let Some(from) = self.front_from.take() else {
            return;
        };
        let fact = Fact::Writer(writer);
        if from.surface == through.surface {
            self.file_since(fact, LineSpan::on(from.surface, from.line, through.line));
            return;
        }
        let end = AbsLine::at(Rows::of_count(usize::MAX));
        self.file_since(fact, LineSpan::on(from.surface, from.line, end));
        let start = AbsLine::at(Rows::ZERO);
        self.file_since(fact, LineSpan::on(through.surface, start, through.line));
    }

    /// **Rows `span` were written by `seen`**, named by the worker after
    /// the program had gone: they keep no mark if it is not local or not
    /// named, and no `./` under a multiplexer; and a `file://` link on them
    /// takes its origin from it ([`Paths::origin_at`]). Only those rows: the
    /// rows above them keep what their own writers made of them. What is
    /// in front now is [`Paths::set_front`]'s, and does not change. A span
    /// of the alternate screen before its last clear is filed nowhere.
    pub fn written_under(&mut self, seen: Seen, span: LineSpan) {
        self.file_since(Fact::of(seen), span);
    }

    /// **Rows `span` were written by `seen`, inside the ssh session
    /// `session`** where the worker read one for the group
    /// ([`Paths::written_under`], then the session filed over them): a
    /// remote program's `file://` link on them opens through that session
    /// ([`PathsAt::session`]), and rows two sessions printed open through
    /// neither.
    pub fn written_by(
        &mut self,
        seen: Seen,
        session: Option<&std::sync::Arc<crate::ssh::Session>>,
        span: LineSpan,
    ) {
        self.written_under(seen, span);
        if let Some(s) = session {
            let printer = self.printer_of(s);
            self.file_since(Fact::Printer(printer), span);
        }
    }

    /// **`session`'s place in the table**, given it the first time, the
    /// same session found again by its value; several once the table is
    /// full, which opens nothing.
    fn printer_of(&mut self, session: &std::sync::Arc<crate::ssh::Session>) -> Printer {
        let at = match self.sessions.iter().position(|s| **s == **session) {
            Some(at) => at,
            None if self.sessions.len() < SESSIONS => {
                self.sessions.push(session.clone());
                self.sessions.len() - 1
            }
            None => return Printer::Several,
        };
        u8::try_from(at).map_or(Printer::Several, |at| Printer::One(SessionAt(at)))
    }

    /// **Which session printed every line `first` to `last` of
    /// `surface`**: one only where each line has a run that holds that
    /// one; none for a line nothing was filed over.
    fn printer_over(&self, surface: Surface, (first, last): (AbsLine, AbsLine)) -> Printer {
        let Some(store) = self.store(surface) else {
            return Printer::None;
        };
        let runs = &store.runs;
        let from = runs.partition_point(|(r, _)| r.last < first);
        let mut got: Option<Printer> = None;
        let mut next = Some(first);
        for (r, facts) in &runs[from..] {
            let Some(want) = next else {
                break;
            };
            if r.first > last {
                break;
            }
            if r.first > want {
                return Printer::None;
            }
            got = Some(got.map_or(facts.printer, |p| p.both(facts.printer)));
            next = (r.last < last).then(|| r.last.below());
        }
        match next {
            Some(_) => Printer::None,
            None => got.unwrap_or_default(),
        }
    }

    /// **A read nothing names wrote `span`**: no group sampled for it and
    /// no marks of the shell's (`probe::Probes::read_taken`). Its rows are
    /// not known, as a group gone before it was named leaves them, and only
    /// those rows -- and not lifted by a command the shell named here
    /// writing them after, which names a writer this read never had.
    pub fn written_unnamed(&mut self, span: LineSpan) {
        self.file_read(Fact::Nameless, span);
    }

    /// **A program not on this machine wrote `span`**, as the job watch or
    /// the shell's marks say of a read (`probe::Watched`, [`crate::ran`]):
    /// its rows keep no mark, and a `file://` link on them takes a remote
    /// origin ([`Paths::origin_at`]). Only those rows, as for any writer.
    pub fn written_remote(&mut self, span: LineSpan) {
        self.file_read(Fact::Writer(Writer::Remote), span);
    }

    /// **The shell named the command that wrote `span` a program on this
    /// machine**, with its token ([`crate::ran::Ran::Here`]): a writer of
    /// those rows the worker could not name is that command's, and local. A
    /// writer named not local, or a multiplexer, keeps what it made of them,
    /// and rows no writer was named for stay not known.
    pub fn written_here(&mut self, span: LineSpan) {
        self.file_read(Fact::Here, span);
    }

    /// **The shell itself wrote `span`**, after its `D` with its token and
    /// with no command open after it ([`crate::ran::Ran::Shell`]): its
    /// prompt after a command, which a read that sampled no group leaves to
    /// this, the shell being on this machine.
    pub fn written_by_shell(&mut self, span: LineSpan) {
        self.file_read(Fact::Writer(Writer::Local), span);
    }

    /// [`Paths::file`], of the rows a read just wrote: those whose facts it
    /// moved are read again at the next scan ([`Reader::refiled`]), as a
    /// read may write a row's facts and none of its cells -- a remote read
    /// of renditions alone over a row holding a `file://` link.
    fn file_read(&mut self, fact: Fact, span: LineSpan) {
        if self.file(fact, span) == Moved::Yes {
            self.refiled = Some(self.refiled.map_or(span, |r| r.joined(span)));
        }
    }

    /// **`fact` filed over `span`**, in its surface's store: each run of
    /// rows it covers gets it added to what was filed there, split at the
    /// span's ends, rows no run held getting a run of their own, and runs
    /// of equal facts that touch merged, so the runs stay sorted, apart and
    /// bounded by how often writers alternate. Whether anything changed;
    /// nothing for a span of the alternate screen before its last clear.
    fn file(&mut self, fact: Fact, span: LineSpan) -> Moved {
        let mut pieces = core::mem::take(&mut self.pieces);
        let moved = match self.store_mut(span.surface) {
            Some(store) => store.file(fact, span, &mut pieces),
            None => Moved::No,
        };
        self.pieces = pieces;
        if moved == Moved::Yes {
            self.origins = self.origins.next();
        }
        moved
    }

    /// [`Paths::file`], of rows written before now: every row is read
    /// again, as an answer moves them, since the fact may change what a
    /// row reads.
    fn file_since(&mut self, fact: Fact, span: LineSpan) {
        if self.file(fact, span) == Moved::Yes {
            self.generation = self.generation.next();
        }
    }

    /// **Every row filed as written by a program on this machine**, for a
    /// test that feeds the terminal itself, with no reader to say who wrote
    /// what: as the reader files the rows of a local program's reads, on
    /// both surfaces.
    #[cfg(test)]
    pub(crate) fn all_written_here(&mut self) {
        for surface in [Surface::Primary, Surface::Alternate(self.alt_epoch)] {
            let all = LineSpan::on(
                surface,
                AbsLine::at(Rows::ZERO),
                AbsLine::at(Rows::of_count(usize::MAX)),
            );
            let _ = self.file(Fact::Writer(Writer::Local), all);
        }
    }

    /// **The facts filed over `line` of `surface`**, found by a binary
    /// search of the runs, the steps it took counted ([`Paths::lookups`]).
    fn facts_at(&self, surface: Surface, line: AbsLine) -> Option<Facts> {
        let runs = &self.store(surface)?.runs;
        let mut steps = Tally::default();
        let i = runs.partition_point(|(r, _)| {
            steps.bump();
            r.last < line
        });
        let mut looked = self.looked.get();
        looked.add(steps);
        self.looked.set(looked);
        runs.get(i)
            .filter(|(r, _)| r.first <= line)
            .map(|&(_, f)| f)
    }

    /// The steps every lookup of a row's facts has taken so far.
    #[cfg(test)]
    fn lookups(&self) -> Tally {
        self.looked.get()
    }

    /// **What wrote `line` of `surface`** ([`Facts::provenance`]); none if
    /// nothing was filed over it, or it is the alternate screen's before its
    /// last clear. A line [`Paths::renumbered`] covers is at least not
    /// known: no local fact filed after lifts it.
    fn wrote(&self, surface: Surface, line: AbsLine) -> Option<Provenance> {
        let filed = self.facts_at(surface, line).and_then(Facts::provenance);
        let floored = self.store(surface).is_some_and(|s| s.floored(line));
        match floored || self.held_over(surface, (line, line)) {
            true => filed.max(Some(Provenance::NotNamed)),
            false => filed,
        }
    }

    /// **The lines of `span` whose facts read as written on this machine**
    /// -- local or a multiplexer's, by what was filed over them alone, held
    /// or not, and not under the floor -- in runs: what a group still to be
    /// named writing over them holds (`probe::Probes::read_taken`, G1),
    /// whoever's verdict lit them. A line nothing was filed over is none of
    /// them: nothing wrote it, and the writer's own answer files it.
    pub fn filed_lit(&self, span: LineSpan) -> Vec<LineSpan> {
        let mut out: Vec<LineSpan> = Vec::new();
        let Some(store) = self.store(span.surface) else {
            return out;
        };
        let runs = &store.runs;
        let from = runs.partition_point(|(r, _)| r.last < span.first);
        for (r, facts) in &runs[from..] {
            if r.first > span.last {
                break;
            }
            let lit = matches!(
                facts.provenance(),
                Some(Provenance::Local | Provenance::Multiplexed)
            );
            let first = r.first.max(span.first);
            let first = match store.floor {
                Some(f) if f >= first => f.below(),
                _ => first,
            };
            let last = r.last.min(span.last);
            if !lit || first > last {
                continue;
            }
            let run = LineSpan::on(span.surface, first, last);
            match out.last_mut() {
                Some(prev) if prev.touches(run) => *prev = prev.joined(run),
                _ => out.push(run),
            }
        }
        out
    }

    /// Whether a held line ([`Paths::set_pending`]) lies in `first` to
    /// `last` of `surface`.
    fn held_over(&self, surface: Surface, (first, last): (AbsLine, AbsLine)) -> bool {
        self.held
            .iter()
            .any(|p| p.surface == surface && p.first <= last && first <= p.last)
    }

    /// Whether `pending` and `held` are the spans [`Paths::set_pending`]
    /// last handed, in whatever order: what a line reads depends on the
    /// lines covered alone.
    #[cfg(test)]
    pub fn pending_is<I, J>(&self, pending: I, held: J) -> bool
    where
        I: Iterator<Item = LineSpan> + Clone,
        J: Iterator<Item = LineSpan> + Clone,
    {
        same_spans(&self.pending, pending) && same_spans(&self.held, held)
    }

    /// **The lines whose writer is still to be named, where their text is
    /// now, and those of them held**, as the paths' worker driver carries
    /// them (`probe::Probes`): a held line reads as not known until its
    /// own writer is named. Where the held lines changed, every line each
    /// covered or covers is read again ([`Reader::refiled`]), and so is a
    /// link's look under the pointer. Where the lines still to be named
    /// changed, [`Reads`] is decided again, and what the last collection
    /// met on them is settled once none of its lines is still to be named
    /// ([`Paths::named`]).
    pub fn set_pending<I, J>(&mut self, pending: I, held: J)
    where
        I: Iterator<Item = LineSpan> + Clone,
        J: Iterator<Item = LineSpan> + Clone,
    {
        if !same_spans(&self.pending, pending.clone()) {
            self.pending.clear();
            self.pending.extend(pending);
            self.named();
        }
        if same_spans(&self.held, held.clone()) {
            return;
        }
        for span in self.held.iter().copied().chain(held.clone()) {
            self.refiled = Some(self.refiled.map_or(span, |r| r.joined(span)));
        }
        self.held.clear();
        self.held.extend(held);
        self.origins = self.origins.next();
    }

    /// **The lines still to be named changed**: [`Reads`] again; and once
    /// none of the lines the last collection met paths on is still to be
    /// named, the batch after their naming is owed for a path where one of
    /// them had no answer kept ([`Awaiting::Unanswered`]) -- the second link
    /// of a listing's chain, which a frame reading no row for paths would
    /// owe only for the rows -- and for nothing more where every answer was
    /// kept, which the naming alone draws.
    fn named(&mut self) {
        self.update_reads();
        let (span, owes) = match self.awaiting {
            Awaiting::Nothing => return,
            Awaiting::Cached(s) => (s, None),
            Awaiting::Unanswered(s) => (s, Some(Owes::Path)),
        };
        if self.pending.iter().any(|p| p.meet(span).is_some()) {
            return;
        }
        self.awaiting = Awaiting::Nothing;
        if let (Some(o), Marks::On) = (owes, self.marks) {
            self.owe(o);
        }
    }

    /// **A resize the driver just made, filed at once**, before any hover,
    /// click or frame reads a line: `rec`, the core's record of it
    /// (`Term::take_record`), taken once for this and the counts
    /// (`keep::kept_across_resize`), and where it says the lines were renumbered,
    /// each surface it renumbered floored ([`Paths::renumbered`]). A re-cut
    /// by a change of columns renumbers the primary's lines, every one up
    /// to its window's last before the resize (`was_last`, [`last_line`])
    /// or after it, numbered by `primary`, its count; a core that recorded
    /// nothing renumbers both, the alternate screen's to its window's last
    /// by `alternate`. Not left for the next read's part, which on a quiet
    /// shell may never come while the old facts sit on the new numbers. A
    /// change of height alone re-cuts nothing and keeps them.
    pub fn resized(
        &mut self,
        rec: &baud_core::ReadRecord,
        term: &Term<'_>,
        was_last: AbsLine,
        primary: Scrolled,
        alternate: Scrolled,
    ) {
        if let Some(why) = rec.renumbered() {
            let last = was_last.max(last_line(term, primary));
            self.part_renumbered(why, (last, last_line(term, alternate)));
        }
    }

    /// **A part the core says renumbered lines, by `why`** (N2, P1): a
    /// re-cut by a change of columns -- a page-width request's resize
    /// among a read's parts, or the window's (`Paths::resized`) --
    /// renumbers the primary's lines up to `primary`, its window's last
    /// that the batch's numbers reached; a core that recorded nothing
    /// renumbers the alternate screen's too, up to `alternate`. Every such
    /// line is not known from here on ([`Paths::renumbered`]).
    pub fn part_renumbered(
        &mut self,
        why: baud_core::Renumbered,
        (primary, alternate): (AbsLine, AbsLine),
    ) {
        self.renumbered(Surface::Primary, primary);
        if why == baud_core::Renumbered::Unrecorded {
            let surface = self.surface(Screen::Alternate);
            self.renumbered(surface, alternate);
        }
    }

    /// **A re-cut of the primary's lines followed line by line** (`map`,
    /// as `Term::resize_with` told it, sorted by the text's lines before),
    /// in place of the floor a re-cut no map follows leaves
    /// ([`Paths::renumbered`]): each line's new lines take the most
    /// cautious writer of the lines its text held ([`Paths::worst_over`],
    /// [`Fact::carried`]), all of them read from the facts as they stood
    /// before, then filed. `at` places the core's numbers here, the same
    /// count before the re-cut and after it (`keep::Counts`, which moves by
    /// the window's top): `old` and `new` are both counted from the old
    /// oldest line, and the new oldest is it plus the lines the re-cut
    /// dropped (`Term::reflow_dropped`), so a line told is the first kept.
    ///
    /// Not known from here on, a local fact filed later included: every
    /// line before the first told -- the lines the re-cut dropped, gone
    /// from the history, and any it never numbered -- by the floor; every
    /// line up to `last` that no line told covers -- the window's last rows
    /// the re-cut blanked, whose cells may come back on a widen, and any
    /// line the map left out -- by a fact no later one lifts
    /// (`Fact::Unvouched`); and every line `at` cannot place, as one whose
    /// writer was named nothing. With nothing told --
    /// a line taller than the whole ring, dropped with the rest -- the
    /// floor reaches `last`, as a re-cut no map follows leaves it.
    ///
    /// Returned: each new line with each span still to be named that
    /// covered its text before (`Paths::pending`), which the driver holds
    /// under that span's group, as for a part's moves ([`Paths::carried`]):
    /// a line two writers' text now shares lights only once both are named.
    pub fn reflowed(
        &mut self,
        map: &[baud_core::LineRecut],
        at: &PartLines,
        last: AbsLine,
    ) -> Vec<(LineSpan, LineSpan)> {
        let surface = Surface::Primary;
        let place = |s: baud_core::Span| {
            let a = at.line(Screen::Normal, s.first())?;
            let b = at.line(Screen::Normal, s.last())?;
            Some(LineSpan::on(surface, a, b))
        };
        let mut recut = core::mem::take(&mut self.recut);
        recut.clear();
        let mut held = Vec::new();
        let mut by = Vec::new();
        let mut printers = Vec::new();
        for r in map {
            // A line before the session's first is no line here.
            let Some(new) = place(r.new) else {
                continue;
            };
            by.clear();
            let (old, worst, printer) = match place(r.old) {
                Some(old) => (
                    old,
                    self.worst_over(surface, (old.first, old.last), &mut by),
                    self.printer_over(surface, (old.first, old.last)),
                ),
                // Before the session's first, as only the oldest can be: on
                // its first line, so the lines before stay in order.
                None => {
                    let first = AbsLine::at(Rows::ZERO);
                    (
                        LineSpan::on(surface, first, first),
                        Some(Provenance::NotNamed),
                        Printer::None,
                    )
                }
            };
            held.extend(by.iter().map(|&p| (new, p)));
            recut.push((old, new, worst.map(Fact::carried)));
            // The session the old lines were all printed under goes with
            // their text.
            if printer != Printer::None {
                printers.push((new, printer));
            }
        }
        // The new lines in order, as the old are: the re-cut keeps the
        // order of the text.
        debug_assert!(recut.windows(2).all(|w| w[0].1.last < w[1].1.first));
        let store = &mut self.primary;
        store.runs.clear();
        store.floor = match recut.first() {
            Some((_, new, _)) => new.first.above(),
            None => Some(last),
        };
        // The lines no line told covers, up to `last`.
        let mut next = recut.first().map(|(_, new, _)| new.first);
        let mut uncovered = Vec::new();
        for &(_, new, _) in &recut {
            if let (Some(n), Some(before)) = (next, new.first.above()) {
                if n <= before {
                    uncovered.push(LineSpan::on(surface, n, before));
                }
            }
            next = Some(new.last.below());
        }
        if let Some(n) = next.filter(|n| *n <= last) {
            uncovered.push(LineSpan::on(surface, n, last));
        }
        for &(_, new, fact) in &recut {
            if let Some(fact) = fact {
                self.file(fact, new);
            }
        }
        for (new, printer) in printers {
            self.file(Fact::Printer(printer), new);
        }
        for span in uncovered {
            self.file(Fact::Unvouched, span);
        }
        self.recut = recut;
        // What else here names a primary line: the program in front's
        // line, the first a `./` is read against the folder from, and each
        // link slot's lines.
        self.front_from = self.front_from.map(|f| self.recut_line(f));
        // Never earlier than it was: a line before the report keeps before
        // it whichever surface the line was read on (`Paths::observe`
        // takes it again at the next scan, the geometry changed).
        self.dir.from = self.dir.from.map(|f| {
            let on = self.recut_line(LineOn { surface, line: f });
            on.line.max(f)
        });
        let mut slots = core::mem::take(&mut self.slots);
        for r in slots.iter_mut().flatten() {
            r.primary = r.primary.map(|s| self.recut_span(s));
        }
        self.slots = slots;
        self.refiled = self.refiled.map(|s| self.recut_span(s));
        self.origins = self.origins.next();
        self.generation = self.generation.next();
        self.changed();
        held
    }

    /// **The line `on` names after the last re-cut** ([`Paths::recut_span`]):
    /// the first of the lines its text now takes. One choice for every
    /// single line carried -- the program in front's, a batch's cursor
    /// (`probe::Probes::recut`) -- so the line a program is named from and
    /// the line the one before it is filed to are the same line: the rows
    /// of that line's text past its first are filed by the spans the reads
    /// that wrote them named.
    pub fn recut_line(&self, on: LineOn) -> LineOn {
        let line = self
            .recut_span(LineSpan::on(on.surface, on.line, on.line))
            .first;
        LineOn { line, ..on }
    }

    /// **Where the text of `span` is after the last re-cut**
    /// ([`Paths::reflowed`]): the new lines of every line told whose text
    /// it held, joined; its lines before the first told, which the re-cut
    /// dropped, on the floor's last line; and its lines past the last told
    /// -- rows no line held -- from the line after the last told on, which
    /// is not known. Never nothing, so a span carried stays a span and
    /// stays cautious. The span as it was on another surface, or where
    /// nothing was told.
    pub fn recut_span(&self, span: LineSpan) -> LineSpan {
        let (Some(&(o0, n0, _)), Some(&(oz, nz, _))) = (self.recut.first(), self.recut.last())
        else {
            return span;
        };
        if span.surface != Surface::Primary {
            return span;
        }
        let mut out: Option<LineSpan> = None;
        let mut join = |s: LineSpan| out = Some(out.map_or(s, |o| o.joined(s)));
        let from = self.recut.partition_point(|(o, _, _)| o.last < span.first);
        for &(o, n, _) in &self.recut[from..] {
            if o.first > span.last {
                break;
            }
            join(n);
        }
        if span.first < o0.first {
            let floor = n0.first.above().unwrap_or(n0.first);
            join(LineSpan::on(span.surface, floor, floor));
        }
        if span.last > oz.last {
            let after = nz.last.below();
            join(LineSpan::on(span.surface, after, span.last.max(after)));
        }
        out.unwrap_or(span)
    }

    /// **The primary's window was taken back past the session's first
    /// line** (`keep::Numbered::PastStart`): its count clamped there while
    /// the core's top fell further, so every line up to `through`, the
    /// window's last before the clamp, now names other text, and is not
    /// known from here on ([`Paths::renumbered`]). A take-back the count
    /// follows keeps every fact on its line.
    pub fn taken_back_past_start(&mut self, through: AbsLine) {
        self.renumbered(Surface::Primary, through);
    }

    /// **Every line of `surface` up to `through` was renumbered in a way no
    /// count says**: a re-cut of the primary's lines by a change of columns
    /// (`Renumbered::Resize`), or a core that recorded nothing
    /// (`Renumbered::Unrecorded`). What was filed over the old numbers
    /// now sits on other text, so it goes, and every line up to `through`
    /// -- the history and the window, each a line the renumbering may have
    /// moved -- is not known from here on, a local fact filed over it
    /// later included: a local prompt redrawn over a remote line the
    /// re-cut moved under it does not light it. Only a later line, which
    /// the renumbering never numbered, is filed as before.
    fn renumbered(&mut self, surface: Surface, through: AbsLine) {
        if let Some(store) = self.store_mut(surface) {
            store.floor = Some(store.floor.map_or(through, |l| l.max(through)));
            store.runs.clear();
        }
        self.origins = self.origins.next();
        self.generation = self.generation.next();
        self.changed();
    }

    /// **Where the program that printed `line` of `surface` runs**: remote
    /// if a program named not local wrote it, local if only local ones did
    /// (a writer not named lifted by the shell's marks included), not known
    /// otherwise -- a writer not named, or nothing filed ([`Paths::wrote`]).
    pub fn origin_at(&self, surface: Surface, line: AbsLine) -> Origin {
        match self.wrote(surface, line) {
            Some(Provenance::Remote) => Origin::Remote,
            Some(Provenance::NotNamed) | None => Origin::Unknown,
            Some(Provenance::Local | Provenance::Multiplexed) => Origin::Local,
        }
    }

    /// [`Paths::origin_at`] on the primary, where most tests file.
    #[cfg(test)]
    pub fn origin(&self, line: AbsLine) -> Origin {
        self.origin_at(Surface::Primary, line)
    }

    /// **Where the program that printed `link` ran, for what it may open**
    /// ([`crate::link::admit_link`]): the most cautious origin over the rows
    /// the link itself covers ([`Paths::origin_over`]) -- an `OSC 8` link's
    /// run, a URI read from the text's own rows -- the one answer its look
    /// at rest, under the pointer, in the preview and at its click all take,
    /// for a link marked with `OSC 8` and for one read from the text alike,
    /// since a `file://` URI a remote program prints as plain text names its
    /// machine's file just as a marked one does. A printed path is asked
    /// about by its own check, and has none. The rows are the screen's up.
    pub fn printed(&self, term: &Term<'_>, link: &crate::link::Under) -> crate::link::Printed {
        use crate::link::{Printed, Under};
        let surface = self.surface(term.screen());
        match link {
            Under::Osc8(o) => Printed::Link {
                origin: match o.uri(term) {
                    Some(uri) => self.link_origin(surface, o.id(), uri, &o.view_rows(term)),
                    None => LinkOrigin::unknown(),
                },
            },
            Under::Bare(b) => Printed::Link {
                origin: self.bare_origin(surface, b.lines()),
            },
            Under::Path(_) => Printed::Path,
        }
    }

    /// **Where the program that printed the `OSC 8` link `id` to `uri` ran**,
    /// the most cautious of two: every row in view holding its slot, `rows`
    /// of `surface` -- the cells its look draws -- and every read that
    /// opened it or wrote under it, filed against the slot on the surface
    /// it wrote ([`Paths::slot_wrote`]). So a link a remote program left
    /// open and a local prompt then wrote under is the remote program's,
    /// and one slot shown over two runs gives either row the same answer. A
    /// record's rows that have left history, or the alternate screen's
    /// before its last clear, read as not known, never local: a link a
    /// local program left open past history stays dark.
    pub fn link_origin(
        &self,
        surface: Surface,
        id: LinkId,
        uri: &[u8],
        rows: &[AbsLine],
    ) -> LinkOrigin {
        // A slot no row in view shows has no row to vouch for it.
        let shown = match rows {
            [] => Origin::Unknown,
            _ => rows.iter().fold(Origin::Local, |o, &line| {
                o.or(self.origin_at(surface, line))
            }),
        };
        let opened = self
            .slots
            .get(id.index())
            .copied()
            .flatten()
            .filter(|r| r.uri == UriHash::of(uri))
            .map_or(Origin::Local, |r| {
                [r.primary, r.alternate]
                    .into_iter()
                    .flatten()
                    .fold(Origin::Local, |o, s| {
                        o.or(self.origin_over(s.surface, (s.first, s.last)))
                    })
            });
        // The session that printed it: the one every row in view and every
        // read that opened it or wrote under it were printed under.
        let record = self
            .slots
            .get(id.index())
            .copied()
            .flatten()
            .filter(|r| r.uri == UriHash::of(uri));
        let opened_by = record
            .into_iter()
            .flat_map(|r| [r.primary, r.alternate])
            .flatten()
            .map(|s| self.printer_over(s.surface, (s.first, s.last)));
        let printer = rows
            .iter()
            .map(|&line| self.printer_over(surface, (line, line)))
            .chain(opened_by)
            .reduce(Printer::both)
            .unwrap_or_default();
        LinkOrigin(shown.or(opened), printer)
    }

    /// **Where the program that printed a URI read from the text on the
    /// lines `first` to `last` of `surface` ran** ([`Paths::origin_over`]).
    pub fn bare_origin(&self, surface: Surface, lines: (AbsLine, AbsLine)) -> LinkOrigin {
        LinkOrigin(
            self.origin_over(surface, lines),
            self.printer_over(surface, lines),
        )
    }

    /// **A part of a read was fed under the link `pen`, writing `span`**:
    /// the slot's record on `span`'s surface joins the span, or, where the
    /// slot holds another target than its record says, starts over at it.
    pub fn slot_wrote(&mut self, pen: Pen, span: LineSpan) {
        let i = pen.id.index();
        if self.slots.len() <= i {
            self.slots.resize(i + 1, None);
        }
        let fresh = SlotRecord {
            uri: pen.uri,
            primary: None,
            alternate: None,
        };
        let joined = match self.slots[i] {
            Some(r) if r.uri == pen.uri => r.joined(span),
            _ => fresh.joined(span),
        };
        if self.slots[i] != Some(joined) {
            self.slots[i] = Some(joined);
            self.generation = self.generation.next();
            self.changed();
        }
    }

    /// **The link table's slots checked against their records** once its
    /// epoch moved: a record is kept only while its slot holds the target
    /// it was filed under, so a slot freed and given another target, or a
    /// reset, forgets it, and the same target given the slot again keeps
    /// it -- the cautious side.
    fn check_slots(&mut self, term: &Term<'_>) {
        let epoch = term.link_epoch();
        if self.slots_at == Some(epoch) {
            return;
        }
        self.slots_at = Some(epoch);
        for (i, slot) in self.slots.iter_mut().enumerate() {
            let held = term.link_uri(LinkId::from_index(i)).map(UriHash::of);
            if slot.is_some_and(|r| Some(r.uri) != held) {
                *slot = None;
            }
        }
    }

    /// **Where the program that printed the lines `first` to `last` of
    /// `surface` ran**: remote if one of them is, local if every one is,
    /// else not known -- a URI read across them is the one program's.
    /// Walked over the runs that touch the lines, not line by line, a line
    /// no run holds not known, so a span a link left open the length of
    /// history costs the runs in it. Not known on the alternate screen of
    /// an epoch past.
    /// **The most cautious writer of lines `first` to `last` of `surface`**
    /// ([`Paths::wrote`]): a line the renumbering covers counted as a writer
    /// not named; a line nothing was filed over carrying nothing (W3) --
    /// every read is filed or still to be named, and the top is always
    /// known, so such a line is one nothing wrote -- and every span still to
    /// be named that touches one pushed to `by`.
    fn worst_over(
        &self,
        surface: Surface,
        (first, last): (AbsLine, AbsLine),
        by: &mut Vec<LineSpan>,
    ) -> Option<Provenance> {
        // A gap carries nothing. One a span still to be named touches: that
        // span is carried with the text (B10), and the line it goes to is
        // held under its group (`Probes::held`) -- every such span, since a
        // gap may join a writer's line to lines below it nothing wrote
        // (F-W3a). Any other is lines nothing wrote (W3), whose move writes
        // nothing the mover did not.
        let mut gap = |a: AbsLine, b: AbsLine| -> Option<Provenance> {
            let touching = self
                .pending
                .iter()
                .filter(|p| p.surface == surface && p.first <= b && a <= p.last);
            by.extend(touching);
            None
        };
        let Some(store) = self.store(surface) else {
            return gap(first, last);
        };
        let runs = &store.runs;
        let from = runs.partition_point(|(r, _)| r.last < first);
        let mut worst = store.floored(first).then_some(Provenance::NotNamed);
        let mut next = Some(first);
        for (r, facts) in &runs[from..] {
            let Some(want) = next else {
                break;
            };
            if r.first > last {
                break;
            }
            if r.first > want {
                worst = worst.max(r.first.above().and_then(|b| gap(want, b)));
            }
            worst = worst.max(Some(facts.provenance().unwrap_or(Provenance::NotNamed)));
            next = (r.last < last).then(|| r.last.below());
        }
        if let Some(want) = next {
            worst = worst.max(gap(want, last));
        }
        worst
    }

    /// **Where the text of `span` is after `rec`'s moves** (B10), for a
    /// span whose writer is still to be named as a part moves its text --
    /// a group a batch has yet to name, the lines it wrote kept until the
    /// answer -- so the answer is filed where the text went, not where it
    /// was read: an exact move shifts the part of the span it moved and
    /// leaves the rest, the hull of both taken; a copy, which replaced part
    /// of each line it wrote (`DECCRA`, [`baud_core::Replaced::Part`]),
    /// leaves the span where it was, since the lines it read keep their
    /// text, and each line it wrote is held under the span
    /// ([`Paths::carried`]); a move within a band takes the band's hull, as
    /// does the fold the record makes of more moves than it keeps. The
    /// span as it was on another surface, or where `lines` places nothing.
    pub fn carried_span(
        &self,
        rec: &baud_core::ReadRecord,
        lines: &PartLines,
        screens: &[Screen],
        span: LineSpan,
    ) -> LineSpan {
        use baud_core::{Move, Replaced, Span};
        let Some(&screen) = screens.iter().find(|&&sc| self.surface(sc) == span.surface) else {
            return span;
        };
        let (Some(a), Some(b)) = (
            lines.core(screen, span.first),
            lines.core(screen, span.last),
        ) else {
            return span;
        };
        let overlaps = |x: Span, y: Span| x.first() <= y.last() && y.first() <= x.last();
        let mut at = Span::of(a, b);
        for m in rec.moves().iter() {
            at = match m {
                Move::Exact { screen: s, .. } | Move::Within { screen: s, .. } if s != screen => at,
                Move::Exact {
                    replaced: Replaced::Part,
                    ..
                } => at,
                Move::Exact { from, by, .. } if overlaps(at, from) => {
                    let inside = Span::of(at.first().max(from.first()), at.last().min(from.last()));
                    match inside.moved(by) {
                        Some(went) if inside == at => went,
                        Some(went) => at.hull(went),
                        None => at.hull(m.band()),
                    }
                }
                Move::Within { band, .. } if overlaps(at, band) => at.hull(band),
                Move::Exact { .. } | Move::Within { .. } => at,
            };
        }
        match (
            lines.line(screen, at.first()),
            lines.line(screen, at.last()),
        ) {
            (Some(a), Some(b)) => LineSpan::on(span.surface, a, b),
            _ => span,
        }
    }

    /// **Text a read moved, filed on the lines it went to** (C2), by the
    /// core's record of it: every line a move wrote to on each of
    /// `screens` takes the most cautious writer of the lines its text may
    /// have come from ([`baud_core::ReadRecord::carried_from`]), itself
    /// among them, each read from the facts as they stood before the read
    /// -- all of them found first, then filed, so a line one move fills is
    /// not read as another's source with what this read put there. A
    /// source the renumbering covers, or whose writer was named nothing,
    /// leaves the line not known, which no fact filed after lifts; a source
    /// nothing was filed over, which nothing wrote, carries nothing (W3); a
    /// line `lines` cannot place is filed nowhere.
    /// The read's own writer is filed over the lines its moves wrote as
    /// over any it wrote (`probe::part_spans`).
    ///
    /// **A line whose text may have come from a line still to be named**
    /// is returned with each span still to be named that covered such a
    /// source, its own line among them: what was filed over the line before
    /// is another text's, and the moved text has no fact until its writer
    /// is named, so the caller holds the line under that span's group
    /// (`Probes::held`) until its answer.
    pub fn carried(
        &mut self,
        rec: &baud_core::ReadRecord,
        lines: &PartLines,
        screens: &[Screen],
    ) -> Vec<(LineSpan, LineSpan)> {
        use baud_core::{Move, Span};
        let mut found = Vec::new();
        let mut held = Vec::new();
        let mut by = Vec::new();
        for &screen in screens {
            let surface = self.surface(screen);
            let mut to: Vec<_> = rec
                .moves()
                .iter()
                .filter(|m| match m {
                    Move::Exact { screen: s, .. } | Move::Within { screen: s, .. } => *s == screen,
                })
                .flat_map(|m| {
                    let band = m.band();
                    core::iter::successors(Some(band.first()), move |l| {
                        (*l < band.last()).then(|| l.next())
                    })
                })
                .collect();
            to.sort_unstable();
            to.dedup();
            for line in to {
                let sources = rec.carried_from(screen, line);
                if sources.iter().all(|s| s == Span::of(line, line)) {
                    continue;
                }
                let Some(here) = lines.line(screen, line) else {
                    continue;
                };
                let here = LineSpan::on(surface, here, here);
                by.clear();
                let worst = sources
                    .iter()
                    .map(
                        |s| match (lines.line(screen, s.first()), lines.line(screen, s.last())) {
                            (Some(a), Some(b)) => self.worst_over(surface, (a, b), &mut by),
                            _ => Some(Provenance::NotNamed),
                        },
                    )
                    .fold(None, Option::max);
                held.extend(by.iter().map(|&p| (here, p)));
                // Only lines whose writer is still to be named: their span
                // is carried to here instead (`Paths::carried_span`).
                let Some(worst) = worst else {
                    continue;
                };
                found.push((here, Fact::carried(worst)));
                // The session every source was printed under goes with the
                // text; text no one session printed, moved onto a line one
                // did, leaves it printed by several.
                let printer = sources
                    .iter()
                    .map(
                        |s| match (lines.line(screen, s.first()), lines.line(screen, s.last())) {
                            (Some(a), Some(b)) => self.printer_over(surface, (a, b)),
                            _ => Printer::None,
                        },
                    )
                    .reduce(Printer::both)
                    .unwrap_or_default();
                let printed_here = self.printer_over(surface, (here.first, here.last));
                match (printer, printed_here) {
                    (Printer::One(_) | Printer::Here, _) => {
                        found.push((here, Fact::Printer(printer)));
                    }
                    (_, Printer::None) => {}
                    (_, _) => found.push((here, Fact::Printer(Printer::Several))),
                }
            }
        }
        for (span, fact) in found {
            self.file_read(fact, span);
        }
        held
    }

    fn origin_over(&self, surface: Surface, (first, last): (AbsLine, AbsLine)) -> Origin {
        let Some(store) = self.store(surface) else {
            return Origin::Unknown;
        };
        let runs = &store.runs;
        let from = runs.partition_point(|(r, _)| r.last < first);
        // A line the renumbering covers is not known ([`Paths::wrote`]).
        let mut all = match store.floored(first) {
            true => Origin::Unknown,
            false => Origin::Local,
        };
        let mut next = Some(first);
        for (r, facts) in &runs[from..] {
            let Some(want) = next else {
                break;
            };
            if r.first > last {
                break;
            }
            if r.first > want {
                all = all.or(Origin::Unknown);
            }
            match facts.provenance() {
                Some(Provenance::Remote) => return Origin::Remote,
                Some(Provenance::NotNamed) | None => all = all.or(Origin::Unknown),
                Some(Provenance::Local | Provenance::Multiplexed) => {}
            }
            next = (r.last < last).then(|| r.last.below());
        }
        if next.is_some() {
            all = all.or(Origin::Unknown);
        }
        // A held line is not known ([`Paths::wrote`]).
        if self.held_over(surface, (first, last)) {
            all = all.or(Origin::Unknown);
        }
        all
    }

    /// **The spans of `screen`'s surface that have left its history
    /// dropped**: those ending above `oldest`, the oldest line it keeps, so
    /// each store is bounded by what its surface keeps.
    pub fn trim_spans(&mut self, screen: Screen, oldest: AbsLine) {
        let surface = self.surface(screen);
        let Some(store) = self.store_mut(surface) else {
            return;
        };
        let was = store.runs.len();
        store.runs.retain(|(r, _)| r.last >= oldest);
        // A link a read on those lines opened is read again: they read as
        // not known now (`Paths::link_origin`).
        if store.runs.len() != was {
            self.generation = self.generation.next();
        }
    }

    /// **Another program is in front than the worker last named**: until it
    /// is named, rows to `through` keep what the last one made of them, and
    /// rows after it are read as the next answer says.
    pub fn front_moved(&mut self, through: LineOn) {
        if self.front == Front::Elsewhere {
            self.leave_front(through);
            self.front = Front::Local;
            self.generation = self.generation.next();
        }
    }

    /// **Nothing is owed**: the program in front was asked about and is the
    /// same, not known to be local.
    pub fn settled(&mut self) {
        self.owing = None;
    }

    /// **A collection starts**: the scan that follows, of every row of the
    /// window, asks about what it finds unanswered; the answers it finds stay
    /// and the rest go, and what it meets on rows still to be named is noted
    /// afresh ([`Awaiting`]).
    pub fn begin_collect(&mut self) {
        self.collect = Some(Collect::default());
        self.unread = Unread::No;
        self.awaiting = Awaiting::Nothing;
        self.update_reads();
    }

    /// **The collection's batch**: what the screen holds that has no answer,
    /// at most [`BATCH_MAX`], each once, single names last and at most
    /// [`NAMES_MAX`] of them. The screen's answers replace the kept ones, so
    /// what is off the screen is forgotten; a screen with more than a batch
    /// stays owed.
    pub fn end_collect(&mut self) -> Vec<Candidate> {
        let Some(mut c) = self.collect.take() else {
            return Vec::new();
        };
        let room = BATCH_MAX.saturating_sub(c.queue.len());
        if c.names.len() > room {
            // Not asked this batch: no longer waiting on an answer.
            for n in c.names.drain(room..) {
                if c.kept.get(&n.path) == Some(&State::Asked) {
                    c.kept.remove(&n.path);
                }
            }
            c.over = Over::Yes;
        }
        c.queue.append(&mut c.names);
        self.known = c.kept;
        self.prefixed = c.prefixed;
        self.marked = c.marked;
        self.owing = match c.over {
            Over::Yes => Some(Owes::Path),
            Over::No => None,
        };
        self.update_reads();
        c.queue
    }

    /// **A batch's answers**: each kept where it was asked, and the
    /// generation moved when one changed, so the lines and the link under the
    /// pointer are read again once a batch. An answer to a path the screen no
    /// longer holds is dropped. The driver tells the lines too
    /// ([`Paths::merge_into`]).
    #[cfg(test)]
    pub fn merge(&mut self, probes: impl IntoIterator<Item = (Vec<u8>, Probe)>) -> Moved {
        match self.merged(probes) {
            Answered::Nothing => Moved::No,
            Answered::Printed | Answered::Everything => Moved::Yes,
        }
    }

    /// **A batch's answers, and `lines` told what they change**: a printed
    /// path's answer redraws the runs `lines` keeps, with no row read
    /// ([`Lines::answered`]); an `OSC 8` link's reads every row again, and so
    /// do answers that start the reading of printed paths ([`Reads`]) after
    /// a row was read since the collection while they were not, since that
    /// row kept no run for them.
    pub fn merge_into(
        &mut self,
        probes: impl IntoIterator<Item = (Vec<u8>, Probe)>,
        lines: &mut Lines,
    ) -> Moved {
        match self.merged(probes) {
            Answered::Nothing => Moved::No,
            Answered::Printed => {
                lines.answered();
                Moved::Yes
            }
            Answered::Everything => {
                lines.forget();
                Moved::Yes
            }
        }
    }

    /// [`Paths::merge`], saying which answers changed.
    fn merged(&mut self, probes: impl IntoIterator<Item = (Vec<u8>, Probe)>) -> Answered {
        let (reads, mut answered) = (self.reads, Answered::Nothing);
        for (path, probe) in probes {
            for (map, changes) in [
                (&mut self.known, Answered::Printed),
                (&mut self.marked, Answered::Everything),
            ] {
                if let Some(s) = map.get_mut(&path) {
                    // An answer asked again that came back the same moves
                    // nothing drawn.
                    if s.probe() != Some(probe) {
                        answered = answered.max(changes);
                    }
                    *s = State::Is(probe);
                }
            }
        }
        if answered != Answered::Nothing {
            self.generation = self.generation.next();
            self.update_reads();
            // Printed paths read from now, with a row read since the
            // collection while they were not: it kept no run to draw.
            if reads != Reads::Yes && (self.reads, self.unread) == (Reads::Yes, Unread::Yes) {
                answered = Answered::Everything;
            }
        }
        answered
    }

    /// **The rows a path on a logical line from `first` to `last` may be
    /// read on**, on `screen`: whether any form is, and whether the relative
    /// ones are, against the folder they would be read against.
    fn readable(&self, text: &Text<'_>, form: Form) -> bool {
        self.readable_on(text.lines, text.screen, form)
    }

    /// [`Paths::readable`], of the lines `first` to `last` on `screen`: every
    /// row of them filed as written on this machine, none by a program not
    /// local or not known, nor by the
    /// program in front since it was named when that is not local; and for
    /// a relative form, none a multiplexer's pane wrote, and a folder to
    /// read it against.
    fn readable_on(&self, (first, last): (AbsLine, AbsLine), screen: Screen, form: Form) -> bool {
        let surface = self.surface(screen);
        // Named on the other surface, the program in front may have written
        // any line of this one.
        let since_front = |f: Front| {
            self.front == f
                && self
                    .front_from
                    .is_none_or(|from| from.surface != surface || last >= from.line)
        };
        if self.marks != Marks::On || since_front(Front::Elsewhere) {
            return false;
        }
        let mut muxed = false;
        let mut line = first;
        loop {
            match self.wrote(surface, line) {
                Some(Provenance::Remote | Provenance::NotNamed) | None => return false,
                Some(Provenance::Multiplexed) => muxed = true,
                Some(Provenance::Local) => {}
            }
            if line >= last {
                break;
            }
            line = line.below();
        }
        if !form.relative() {
            return true;
        }
        if muxed || since_front(Front::Multiplexer) {
            return false;
        }
        match Resolve::on(screen) {
            Resolve::Reported => {
                self.dir.local.is_some() && self.dir.from.is_some_and(|f| first >= f)
            }
            Resolve::Program => self.program_dir.is_some(),
        }
    }

    /// The folder a relative path on `screen` is read against, if any.
    fn folder(&self, screen: Screen) -> Option<&[u8]> {
        match Resolve::on(screen) {
            Resolve::Reported => self.dir.local.as_deref(),
            Resolve::Program => self.program_dir.as_ref().map(|d| d.0.as_slice()),
        }
    }

    /// The path `token` names on `screen`, resolved, into `self.key`; false
    /// when it cannot be: no home, no folder.
    fn resolve(&mut self, units: &[Unit], token: &Token, screen: Screen) -> bool {
        self.key.clear();
        let text = &units[token.path.clone()];
        match token.form {
            Form::Absolute => {}
            Form::Home => {
                let Some(h) = &self.home else { return false };
                self.key.extend_from_slice(h);
                push_utf8(&text[1..], &mut self.key);
                return true;
            }
            Form::Dot | Form::Parent | Form::Bare | Form::Name | Form::Word => {
                // `Paths::folder`, spelled over the fields: `self.key` is
                // borrowed beside it.
                let folder = match Resolve::on(screen) {
                    Resolve::Reported => self.dir.local.as_deref(),
                    Resolve::Program => self.program_dir.as_ref().map(|d| d.0.as_slice()),
                };
                let Some(d) = folder else {
                    return false;
                };
                self.key.extend_from_slice(d);
                if self.key.last() != Some(&b'/') {
                    self.key.push(b'/');
                }
            }
        }
        push_utf8(below_folder(token.form, text), &mut self.key);
        true
    }

    /// **The scan's half**: the paths in `text` its rows may hold, each
    /// drawn where the worker found it, and each it has no answer for owed
    /// or, in a collection, asked. Called only while [`Reads`] is yes. A path over a URI already found is not
    /// one. Returns how many it looked at.
    fn read(&mut self, text: Text<'_>, found: &mut Vec<Run>) -> Looked {
        self.read_keys.clear();
        let mut toks = core::mem::take(&mut self.tokens);
        tokens(text.units, (text.head, text.tail), &self.forms, &mut toks);
        let uris = found.len();
        let mut looked = 0;
        for t in &toks {
            let shown = t.span.start < text.shown.1 && text.shown.0 < t.span.end;
            let over = found[..uris]
                .iter()
                .any(|f| f.units.start < t.span.end && t.span.start < f.units.end);
            if !shown || over {
                continue;
            }
            if !self.readable(&text, t.form) {
                // In front of a program not known to be local: whether it
                // still is gets asked once the screen settles.
                if self.front == Front::Elsewhere && self.marks == Marks::On {
                    self.owe(Owes::Path);
                } else if let (Some(span), Marks::On) =
                    (self.pending_on(text.lines, text.screen), self.marks)
                {
                    let answer =
                        match self.collect.is_some() && self.resolve(text.units, t, text.screen) {
                            true => self.known.get(self.key.as_slice()).copied(),
                            false => None,
                        };
                    self.awaits_naming(answer, span);
                }
                continue;
            }
            if !self.resolve(text.units, t, text.screen) {
                continue;
            }
            looked += 1;
            let state = self.known.get(self.key.as_slice()).copied();
            if let Some(c) = &mut self.collect {
                if !t.form.single() && !c.prefixed.contains(self.key.as_slice()) {
                    c.prefixed.insert(self.key.clone());
                }
                let room = match t.form.single() {
                    true => c.names.len() < NAMES_MAX,
                    false => c.queue.len() < BATCH_MAX,
                };
                if c.kept.get(self.key.as_slice()) == Some(&State::Asked) {
                    // Printed again lower down: asked at its last line.
                    if let Some(q) = c
                        .queue
                        .iter_mut()
                        .chain(c.names.iter_mut())
                        .find(|q| q.path == self.key)
                    {
                        q.line = q.line.max(text.lines.1);
                    }
                } else if !c.kept.contains_key(self.key.as_slice()) {
                    match state {
                        Some(State::Is(p)) => {
                            c.kept.insert(self.key.clone(), State::Is(p));
                        }
                        s if room => {
                            let s = match s {
                                Some(State::Stale(p)) => State::Stale(p),
                                _ => State::Asked,
                            };
                            c.kept.insert(self.key.clone(), s);
                            let asked = Candidate {
                                path: self.key.clone(),
                                form: t.form,
                                line: text.lines.1,
                            };
                            match t.form.single() {
                                true => c.names.push(asked),
                                false => c.queue.push(asked),
                            }
                        }
                        Some(s @ State::Stale(_)) => {
                            c.kept.insert(self.key.clone(), s);
                            c.over = Over::Yes;
                        }
                        Some(State::Asked) | None => c.over = Over::Yes,
                    }
                }
            }
            let probe = state.and_then(State::probe);
            if probe.is_none() && self.collect.is_none() && state.is_none() {
                self.owe(Owes::Path);
            }
            // Drawn where the worker found it; kept either way, so an answer
            // draws it with no row read (`Lines::answered`). Only one with an
            // answer kept or asked for can get one: a path asked for later is
            // asked by a collection, which reads every row again.
            if (state.is_some() || self.collect.is_some())
                && self.forms.at_rest(t.form)
                && opens_as_uri(&self.key)
            {
                let k = self.read_keys.len();
                self.read_keys.extend_from_slice(&self.key);
                found.push(Run {
                    units: t.span.clone(),
                    mark: Mark::Line,
                    key: Key::Path {
                        at: k..self.read_keys.len(),
                        drawn: match probe {
                            Some(Probe::Exists) => Drawn::Yes,
                            _ => Drawn::No,
                        },
                    },
                });
            }
        }
        self.tokens = toks;
        Looked(looked)
    }

    /// **An `OSC 8` `file://` link at `at`, which `admit` opens**: struck
    /// where its host names this machine by name and the worker found its
    /// file gone, else a line; in a collection, asked about once a screen,
    /// and outside one owed when it has no answer. Nothing is asked while
    /// `missing` is off, for output not known to be this machine's, or for
    /// a name [`file_path`] refuses.
    fn file_link(&mut self, uri: &[u8], at: At<'_>) -> Mark {
        if self.marks != Marks::On || self.missing != Turned::On {
            return Mark::Line;
        }
        let Some((named, path)) = file_path(uri, &self.names) else {
            return Mark::Line;
        };
        if !self.readable_on(at.span(), at.screen(), Form::Absolute) {
            if self.front == Front::Elsewhere {
                self.owe(Owes::Path);
            }
            return Mark::Line;
        }
        let state = self.marked.get(path.as_slice()).copied();
        if let Some(c) = &mut self.collect {
            if c.marked.get(path.as_slice()) == Some(&State::Asked) {
                // Linked again lower down: asked at its last line.
                if let Some(q) = c.queue.iter_mut().find(|q| q.path == path) {
                    q.line = q.line.max(at.line());
                }
            } else if !c.marked.contains_key(path.as_slice()) {
                match state {
                    Some(State::Is(p)) => {
                        c.marked.insert(path.clone(), State::Is(p));
                    }
                    s if c.queue.len() < BATCH_MAX => {
                        let s = match s {
                            Some(State::Stale(p)) => State::Stale(p),
                            _ => State::Asked,
                        };
                        c.marked.insert(path.clone(), s);
                        c.queue.push(Candidate {
                            path: path.clone(),
                            form: Form::Absolute,
                            line: at.line(),
                        });
                    }
                    Some(s) => {
                        c.marked.insert(path.clone(), s);
                        c.over = Over::Yes;
                    }
                    None => c.over = Over::Yes,
                }
            }
        } else if state.is_none() {
            self.owe(Owes::Path);
        }
        match (named, state.and_then(State::probe)) {
            (Named::ThisMachine, Some(Probe::Missing)) => Mark::Struck,
            _ => Mark::Line,
        }
    }

    /// Whether a line ending in `tail`, its last eight ASCII bytes, the
    /// newest last, may hold a path worth reading: under
    /// [`Reads::Pending`], only until a path is owed, so a flood still to be
    /// named has its lines read for paths by one frame a batch, not each.
    fn wants(&self, tail: [u8; 8]) -> bool {
        let reads = match self.reads {
            Reads::Yes => true,
            Reads::Pending => self.owing != Some(Owes::Path),
            Reads::No => false,
        };
        reads && starts(tail, &self.forms)
    }

    /// **An `OSC 8` link at `at` its click refuses**, which is a `file://`
    /// link's on lines still to be named: owed for its file as
    /// [`Paths::file_link`] owes one there, the naming deciding the rest.
    fn refused_link(&mut self, uri: &[u8], at: At<'_>) {
        if self.marks != Marks::On || self.missing != Turned::On || self.front == Front::Elsewhere {
            return;
        }
        let Some((_, path)) = file_path(uri, &self.names) else {
            return;
        };
        if let Some(span) = self.pending_on(at.span(), at.screen()) {
            let answer = self.marked.get(path.as_slice()).copied();
            self.awaits_naming(answer, span);
        }
    }

    /// The lines `first` to `last` on `screen`, where they meet a line
    /// still to be named.
    fn pending_on(&self, (first, last): (AbsLine, AbsLine), screen: Screen) -> Option<LineSpan> {
        let span = LineSpan::on(self.surface(screen), first, last);
        self.pending
            .iter()
            .any(|p| p.meet(span).is_some())
            .then_some(span)
    }

    /// **A path on `span`, lines still to be named**, printed or an `OSC 8`
    /// `file://` link's, `answer` its kept answer as a collection reads it:
    /// their naming makes it readable, so a frame owes the batch that names
    /// them for a path ([`Owes::Path`]); a collection notes, for after that
    /// naming, whether its answer is kept ([`Awaiting`]).
    fn awaits_naming(&mut self, answer: Option<State>, span: LineSpan) {
        match self.collect.is_some() {
            false => self.owe(Owes::Path),
            true => self.awaiting = self.awaiting.with(answer, span),
        }
    }

    /// **The scan's half while only lines still to be named are read**
    /// ([`Reads::Pending`]): a batch owed for a path where `text` meets
    /// such a line and holds one outside the URIs in `found`, which the
    /// naming makes readable. Nothing is drawn, kept or resolved; and once
    /// a path is owed, no line is tokenized for it until a batch goes.
    fn read_pending(&mut self, text: Text<'_>, found: &[Run]) {
        if self.owing == Some(Owes::Path) || self.pending_on(text.lines, text.screen).is_none() {
            return;
        }
        let mut toks = core::mem::take(&mut self.tokens);
        tokens(text.units, (text.head, text.tail), &self.forms, &mut toks);
        let met = toks.iter().any(|t| {
            let shown = t.span.start < text.shown.1 && text.shown.0 < t.span.end;
            let over = found
                .iter()
                .any(|f| f.units.start < t.span.end && t.span.start < f.units.end);
            shown && !over
        });
        self.tokens = toks;
        if met {
            self.owe(Owes::Path);
        }
    }
}

/// **The window's last line**, numbered by `scrolled`: what a driver reads
/// before a resize, for [`Paths::resized`].
pub fn last_line(term: &Term<'_>, scrolled: Scrolled) -> AbsLine {
    let rows = term.geometry().1;
    scrolled.line(
        Rows::ZERO,
        baud_domain::Row::from_index(rows.index().saturating_sub(1)),
    )
}

/// Whether every row from just below `top` to `bottom` is blank: what `clear`
/// leaves under its prompt.
fn blank_between(term: &Term<'_>, scrolled: Scrolled, top: AbsLine, bottom: AbsLine) -> bool {
    let mut line = top.below();
    while line <= bottom {
        let Some((cells, _)) = row_of(term, scrolled, line) else {
            return true;
        };
        let blank = cells.iter().all(|c| {
            c.content
                .as_codepoint()
                .is_none_or(|p| p.value() == u32::from(b' '))
        });
        if !blank {
            return false;
        }
        line = line.below();
    }
    true
}

/// **The link under the pointer that is a printed path**, for a click and the
/// link preview: what [`PathsAt::at`] answers over a click's window of text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PathAt {
    /// Its units in the window's text, the suffix included.
    pub span: Range<usize>,
    /// The suffix, as digits.
    pub line_col: LineCol,
    /// The path as resolved, which a click asks about again.
    pub path: Vec<u8>,
    pub form: Form,
    /// What the worker last said: only a path it found or did not.
    pub probe: Probe,
}

/// **What a click asked the worker about again**: a printed path, with the
/// `:line:col` printed after it, or a link whose file the worker had found
/// gone, which only a link that reveals is (`link::click`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Asked {
    Printed(LineCol),
    Link,
}

/// **The ask the answer for `path` came from**: the click's own, kept in
/// `slot`, when it asked about `path`, else a link's reveal, which runs
/// nothing of `[link.path]`'s. The worker answers only the latest click
/// (`probe::Probes::click`) and with the path it was given, so an answer
/// is the slot's own whenever the paths match; one that does not is an
/// earlier ask's, and never takes a later printed path's copy or command.
pub fn asked_for(slot: Option<(Vec<u8>, Asked)>, path: &[u8]) -> Asked {
    slot.filter(|(p, _)| p == path)
        .map_or(Asked::Link, |(_, a)| a)
}

/// What a click asked again does once its file is found.
#[derive(Debug)]
pub enum Found {
    /// `[link.path] action = "copy"`: the path to the clipboard.
    Copy,
    /// `[link.path]`'s command, built for the path.
    Command(Result<std::process::Command, crate::link::Refused>),
    /// `[link.path] action = "window"`: the argv a new window runs for the
    /// path, built for it as a command's is.
    Window(
        #[cfg_attr(not(test), allow(dead_code))] Result<crate::rules::Run, crate::link::Refused>,
    ),
    /// Shown in the file manager, through `admit` again.
    Reveal,
}

/// **What a click asked again does, its file found**: a printed path takes
/// `[link.path]`'s copy, command or window, else it is revealed; a link
/// takes its own action, a reveal (a rule's copy, command or window runs
/// before any asking, `link::click`, and the reader refuses a file opened
/// by the system), and never `[link.path]`'s, which is for printed paths.
/// No found file is opened by the system. `host` is what `admit` reads a
/// `file://` host against.
pub fn on_found(
    asked: &Asked,
    path: &[u8],
    host: &crate::link::MachineNames,
    rules: &Rules,
) -> Found {
    let Asked::Printed(lc) = asked else {
        return Found::Reveal;
    };
    if *rules.path_act() == crate::rules::PathAct::Copy {
        return Found::Copy;
    }
    let uri = file_uri(path);
    if crate::link::admit_path(uri.as_bytes(), host).is_err() {
        return Found::Reveal;
    }
    if let Some(built) = rules.path_command(path, &lc.line, &lc.col) {
        return Found::Command(built);
    }
    match rules.path_window(path, &lc.line, &lc.col) {
        Some(built) => Found::Window(built),
        None => Found::Reveal,
    }
}

/// **The `:line:col` printed after a path**, as its digits, each empty
/// where none was: what `[link.path]`'s `{line}` and `{col}` take.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct LineCol {
    pub line: String,
    pub col: String,
}

impl LineCol {
    /// The digits of `suffix`, a path's `:line` or `:line:col`.
    fn of(suffix: &[Unit]) -> LineCol {
        let mut parts = suffix.split(|u| *u == Unit::Char(':')).skip(1).map(|p| {
            p.iter()
                .filter_map(|u| match u {
                    Unit::Char(c) if c.is_ascii_digit() => Some(*c),
                    _ => None,
                })
                .collect::<String>()
        });
        LineCol {
            line: parts.next().unwrap_or_default(),
            col: parts.next().unwrap_or_default(),
        }
    }
}

/// **What a read of the link under the pointer asks of printed paths**: the
/// seam a driver hands the preview and the click, which only read.
pub trait PathsAt {
    /// Moved whenever an answer would read otherwise.
    fn generation(&self) -> Generation;

    /// **The `[link.*]` rules the read goes by**: a scheme a rule of yours
    /// added is read in the text and admitted as baud's own are. Held here
    /// because the forms a path is read in are among them; none by default.
    fn rules(&self) -> &Rules {
        &NO_RULES
    }

    /// **Whether a read may join the row the writer ended at `above` to the
    /// next**, where it runs on in a URI's characters (`link::runs_on`):
    /// `[link] join_rows`, and the two rows' provenance. Yes with nothing to
    /// go by, as `join_rows` is on by default.
    fn joins(&self, _screen: Screen, _above: AbsLine) -> Joins {
        Joins::Yes
    }

    /// Moved whenever a line's provenance may have, which may move
    /// [`PathsAt::joins`] with no byte and no answer of the worker's.
    fn origins(&self) -> Generation {
        Generation::default()
    }

    /// **Where the program that printed `link` ran**, over the rows it
    /// covers, for what the link looks like and does ([`Paths::printed`]). No default: a
    /// reader with no worker to have named a writer says what it takes the
    /// line for.
    fn printed(&self, term: &Term<'_>, link: &crate::link::Under) -> crate::link::Printed;

    /// The path at unit `at` of a click's window of text, `lines` its first
    /// and last line, on `screen`, if the worker found it or found it gone.
    fn at(
        &self,
        text: &[Unit],
        at: Ix,
        edges: (Edge, Edge),
        lines: (AbsLine, AbsLine),
        screen: Screen,
    ) -> Option<PathAt>;

    /// **Why `target`, which `admit` lets through, would open nothing**, as
    /// the worker last found: its file gone from this machine, whatever its
    /// host names it by, or no app for its kind. None where it would open
    /// or baud cannot tell.
    fn gone(&self, _target: &Target<'_>) -> Option<Gone> {
        None
    }

    /// Whether anything is struck, under the chord as at rest: `[link]
    /// missing`, with a worker to have asked.
    fn strikes(&self) -> bool {
        false
    }

    /// **The ssh session that printed a link judged `origin`**, which a
    /// remote program's `file://` link opens through (`link::remote`):
    /// the one session filed over every row the link was printed on, never
    /// the one in front at the click; none with no worker to have read one.
    fn session(&self, _origin: LinkOrigin) -> Option<&crate::ssh::Session> {
        None
    }
}

/// No printed paths: what every read had before them. The drivers read
/// through their [`Paths`], so only tests read through this.
#[cfg(test)]
pub struct NoPaths;

#[cfg(test)]
impl PathsAt for NoPaths {
    fn generation(&self) -> Generation {
        Generation::default()
    }

    /// With no worker, every line is taken as this machine's: what the
    /// tests that read through this take a link's line for.
    fn printed(&self, _: &Term<'_>, link: &crate::link::Under) -> crate::link::Printed {
        use crate::link::{Printed, Under};
        match link {
            Under::Path(_) => Printed::Path,
            Under::Osc8(_) | Under::Bare(_) => Printed::Link {
                origin: LinkOrigin(Origin::Local, Printer::None),
            },
        }
    }

    fn at(
        &self,
        _: &[Unit],
        _: Ix,
        _: (Edge, Edge),
        _: (AbsLine, AbsLine),
        _: Screen,
    ) -> Option<PathAt> {
        None
    }
}

impl PathsAt for Paths {
    fn generation(&self) -> Generation {
        self.generation
    }

    fn printed(&self, term: &Term<'_>, link: &crate::link::Under) -> crate::link::Printed {
        Paths::printed(self, term, link)
    }

    /// On, both rows of one provenance: written while the same kind of
    /// program was in front, this machine's, another's or one not named
    /// yet ([`Paths::origin`]), so a remote line never runs into a local one.
    fn origins(&self) -> Generation {
        self.origins
    }

    fn joins(&self, screen: Screen, above: AbsLine) -> Joins {
        let surface = self.surface(screen);
        let origin = |line| self.origin_at(surface, line);
        match self.join {
            Turned::On if origin(above) == origin(above.below()) => Joins::Yes,
            Turned::On | Turned::Off => Joins::No,
        }
    }

    fn rules(&self) -> &Rules {
        &self.rules
    }

    fn session(&self, origin: LinkOrigin) -> Option<&crate::ssh::Session> {
        if self.marks == Marks::Closed {
            return None;
        }
        match origin.1 {
            Printer::One(at) => self.sessions.get(usize::from(at.0)).map(|s| &**s),
            Printer::None | Printer::Here | Printer::Several => None,
        }
    }

    fn at(
        &self,
        text: &[Unit],
        at: Ix,
        edges: (Edge, Edge),
        lines: (AbsLine, AbsLine),
        screen: Screen,
    ) -> Option<PathAt> {
        if self.marks != Marks::On {
            return None;
        }
        let mut toks = Vec::new();
        tokens(text, edges, &self.forms, &mut toks);
        let t = toks.into_iter().find(|t| t.span.contains(&at.0))?;
        let probe_text = Text {
            units: text,
            head: edges.0,
            tail: edges.1,
            shown: (0, text.len()),
            lines,
            points: &[],
            screen,
        };
        if !self.readable(&probe_text, t.form) {
            return None;
        }
        // `resolve` without its scratch, which this read may not take.
        let mut key = Vec::new();
        let body = &text[t.path.clone()];
        match t.form {
            Form::Absolute => push_utf8(body, &mut key),
            Form::Home => {
                key.extend_from_slice(self.home.as_deref()?);
                push_utf8(&body[1..], &mut key);
            }
            Form::Dot | Form::Parent | Form::Bare | Form::Name | Form::Word => {
                key.extend_from_slice(self.folder(screen)?);
                if key.last() != Some(&b'/') {
                    key.push(b'/');
                }
                push_utf8(below_folder(t.form, body), &mut key);
            }
        }
        let probe = match self.known.get(&key).and_then(|s| s.probe()) {
            Some(p @ (Probe::Exists | Probe::Missing)) => p,
            _ => return None,
        };
        Some(PathAt {
            line_col: LineCol::of(&text[t.path.end..t.span.end]),
            span: t.span,
            path: key,
            form: t.form,
            probe,
        })
    }

    fn strikes(&self) -> bool {
        self.marks == Marks::On && self.missing == Turned::On
    }

    fn gone(&self, target: &Target<'_>) -> Option<Gone> {
        if !self.strikes() {
            return None;
        }
        match target.scheme() {
            Scheme::File => {
                // The file the policy read, not a second parse of the text.
                let path = target.local_file()?.path()?;
                match self.marked.get(path.as_slice()).and_then(|s| s.probe()) {
                    Some(Probe::Missing) => Some(Gone::File(path)),
                    _ => None,
                }
            }
            s => match (self.rule_act(target), self.target_mark(target)) {
                (_, Mark::Line) => None,
                (
                    Some(crate::rules::Act::Command(_) | crate::rules::Act::Window(_)),
                    Mark::Struck,
                ) => Some(Gone::NoCommand),
                (_, Mark::Struck) => Some(Gone::NoApp(s)),
            },
        }
    }
}

/// **URIs and printed paths, in one pass**: the reader both drivers scan
/// with. URIs are read first, and a path is read only outside them; the runs
/// come back in order, none overlapping, as [`Reader::read`] asks.
pub struct Links<'a> {
    pub uris: &'a mut Uris,
    /// `[link] urls`.
    pub urls: Turned,
    pub paths: &'a mut Paths,
}

impl Reader for Links<'_> {
    #[inline]
    fn wants(&self, tail: crate::rules::Tail) -> bool {
        (self.urls == Turned::On && self.uris.wants(tail)) || self.paths.wants(tail.0.to_be_bytes())
    }

    fn joins(&self, screen: Screen, above: AbsLine) -> Joins {
        PathsAt::joins(&*self.paths, screen, above)
    }

    fn key(&self, at: Range<usize>) -> &[u8] {
        self.paths.read_keys.get(at).unwrap_or_default()
    }

    fn drawn(&self, key: &[u8]) -> Drawn {
        match self.paths.known.get(key).and_then(|s| s.probe()) {
            Some(Probe::Exists) => Drawn::Yes,
            _ => Drawn::No,
        }
    }

    fn read(&mut self, text: Text<'_>, found: &mut Vec<Run>) -> Looked {
        let mut n = 0;
        if self.urls == Turned::On {
            let paths = &*self.paths;
            // A plain `file://` URI a program not local, or not known,
            // printed on any of the lines it covers names its machine's
            // file: unmarked, as its click refuses it
            // ([`crate::link::admit_link`]), unless the click opens it on
            // the machine of the session that printed it
            // ([`crate::link::remote`]).
            let surface = paths.surface(text.screen);
            let origin = |lines| paths.bare_origin(surface, lines);
            let session = |o| PathsAt::session(paths, o);
            n += self
                .uris
                .read_marking(text, found, &origin, &session, &|t| {
                    Some(paths.target_mark(t))
                })
                .0;
        }
        match self.paths.reads {
            Reads::Yes => {
                n += self.paths.read(text, found).0;
                found.sort_unstable_by_key(|r| r.units.start);
            }
            Reads::Pending => self.paths.read_pending(text, found),
            Reads::No => {}
        }
        Looked(n)
    }

    fn marked(&mut self, uri: &[u8], at: At<'_>) -> Option<Mark> {
        if self.urls != Turned::On {
            return None;
        }
        // A `file://` link a program not local, or not known, printed on any
        // row its slot is shown on, or a read that opened it wrote, names its
        // machine's file: unmarked, as its click refuses it.
        let surface = self.paths.surface(at.screen());
        let origin = self.paths.link_origin(surface, at.id(), uri, at.rows());
        let (names, rules) = (self.uris.names(), self.uris.rules());
        // One its click opens on the machine of the session that printed
        // it is lined, and one that opens nothing there is not.
        let session = PathsAt::session(&*self.paths, origin);
        if let Some(window) = crate::link::remote(uri, names, rules, origin, session) {
            return window.ok().map(|_| Mark::Line);
        }
        let Ok(t) = admit_link(uri, names, rules, origin) else {
            // Refused while its rows' writer is still to be named: that
            // naming may admit it, and a `file://` link's file is asked.
            self.paths.refused_link(uri, at);
            return None;
        };
        Some(match t.scheme() {
            Scheme::File => self.paths.file_link(uri, at),
            _ => self.paths.target_mark(&t),
        })
    }

    fn changed(&mut self) {
        self.paths.changed();
    }

    fn refiled(&mut self) -> Option<LineSpan> {
        self.paths.refiled.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lined::Lines;
    use crate::link::admit;
    use crate::tool::tool;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

    /// **Each part's lines are placed against its own window top**
    /// (`CoreLines::part`): a batch begun with the core's top at line 10,
    /// numbered 3 here. A part whose top rose to 12 places line 12 at 5
    /// and line 9 at 2; one whose top fell to 8 places line 8 at 1; one
    /// whose top fell to 6, past the count's first line, places nothing,
    /// and neither does a line before the first under any part. Named
    /// break: number a part's top by the batch's count as it began, as
    /// one top read after the batch would number a part before a re-cut
    /// -- line 12 under the part at 12 lands on 3.
    #[test]
    fn each_part_is_placed_against_its_own_top() {
        use baud_domain::LineIdx;
        let at = |n: usize| AbsLine::at(Rows::of_count(n));
        let core = |n: u64| LineIdx::of(n);
        let batch = CoreLines::of(Some((
            core(10),
            Scrolled::START.advanced(Rows::of_count(3)),
        )));
        let up = batch.part(Some(core(12)));
        assert_eq!(up.line(Screen::Normal, core(12)), Some(at(5)));
        assert_eq!(up.line(Screen::Normal, core(9)), Some(at(2)));
        assert_eq!(up.line(Screen::Normal, core(6)), None);
        let down = batch.part(Some(core(8)));
        assert_eq!(down.line(Screen::Normal, core(8)), Some(at(1)));
        assert_eq!(
            batch.part(Some(core(6))).line(Screen::Normal, core(6)),
            None
        );
        assert_eq!(batch.part(None).line(Screen::Normal, core(10)), None);
    }

    const WS: WinSize = WinSize {
        ws_col: 60,
        ws_row: 6,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        for c in bytes.chunks(4096) {
            t.write_at(BytePos::ZERO, c);
            t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
        }
    }

    fn at() -> Scrolled {
        Scrolled::START.advanced(Rows::of_count(1_000))
    }

    /// `line` on the primary, where most tests file.
    fn on_primary(line: AbsLine) -> LineOn {
        LineOn {
            surface: Surface::Primary,
            line,
        }
    }

    fn units(s: &str) -> Vec<Unit> {
        s.chars().map(Unit::Char).collect()
    }

    fn found(s: &str, forms: &Forms) -> Vec<String> {
        let u = units(s);
        let mut out = Vec::new();
        tokens(&u, (Edge::Line, Edge::Line), forms, &mut out);
        out.iter()
            .map(|t| {
                u[t.path.clone()]
                    .iter()
                    .map(|x| match x {
                        Unit::Char(c) => *c,
                        _ => '?',
                    })
                    .collect()
            })
            .collect()
    }

    fn runs(l: &Lines) -> Vec<(usize, usize, usize)> {
        l.lines()
            .iter()
            .map(|s| (s.row.index(), s.from.index(), s.to.index()))
            .collect()
    }

    /// A window's paths, with `exists` answered as there and the rest as not.
    fn scanned(bytes: &[u8], exists: &[&str]) -> (Vec<(usize, usize, usize)>, Paths) {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, bytes);
        let mut p = Paths::new(Some(b"/home/user".to_vec()), Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = p.observe(&t, at());
        p.begin_collect();
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::On,
                paths: &mut p,
            },
        );
        let asked = p.end_collect();
        let answers = asked.into_iter().map(|c| {
            let there = exists.iter().any(|e| e.as_bytes() == c.path.as_slice());
            (c.path, if there { Probe::Exists } else { Probe::Missing })
        });
        let _ = p.merge_into(answers, &mut l);
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::On,
                paths: &mut p,
            },
        );
        (runs(&l), p)
    }

    /// **The default forms are `/`, `~/`, `./`, `../` and a relative path**,
    /// after an opener, and never inside a URI or after `host:`; trailing
    /// prose and an unopened bracket are cut, a lone prefix is no path, and
    /// a path after `=` is read whatever is before it. The other forms are
    /// off until turned on, and each reads once it is. Named breaks: the old
    /// default, `relative` off -- `src/app.rs` is not read; read `:` as an
    /// opener; drop the length check after the prefix; read a lone `./` as a
    /// relative path.
    #[test]
    fn the_default_forms_are_the_prefixes_and_relative_paths() {
        let d = Forms::DEFAULT;
        assert_eq!(
            found("see /etc/hosts, and ~/notes.txt.", &d),
            ["/etc/hosts", "~/notes.txt"]
        );
        assert_eq!(
            found("run ./build.sh (or /usr/bin/x)", &d),
            ["./build.sh", "/usr/bin/x"]
        );
        assert_eq!(found("https://a.example/b host:/x a/b /", &d), ["a/b"]);
        assert_eq!(
            found("../up src/app.rs /x/y.rs:12:3", &d),
            ["../up", "src/app.rs", "/x/y.rs:12:3"]
        );
        assert_eq!(
            found("//server/share ~/ ./ ../ .. ./..", &d),
            Vec::<String>::new()
        );
        assert_eq!(found("--file=/tmp/a \"/tmp/b\"", &d), ["/tmp/a", "/tmp/b"]);
        let all = Forms {
            parent: Turned::On,
            bare: Turned::On,
            line_col: Turned::On,
            spaces: Turned::On,
            ..d
        };
        assert_eq!(
            found("../up src/app.rs /x/y.rs:12:3 host:a/b and", &all),
            ["../up", "src/app.rs", "/x/y.rs"]
        );
        let spaces = Forms {
            spaces: Turned::On,
            ..d
        };
        assert_eq!(found("open \"/tmp/a b/c\" now", &spaces), ["/tmp/a b/c"]);
        assert_eq!(found("open \"/tmp/a b/c now", &spaces), ["/tmp/a", "b/c"]);
        assert_eq!(
            found("open \"/tmp/a b/c\" now", &d),
            ["/tmp/a", "b/c"],
            "off: a space ends it"
        );
    }

    /// **A relative path ends before a `=` and at a list's `:`, and a
    /// prefixed path wins there**: `--file=/tmp/a` is `/tmp/a`, never one
    /// relative path holding it, and `--out=build/x` is `build/x`; a name
    /// after a `=` is the name alone; `a/b:/c/d` and `a/b:~/c` are two
    /// paths each, while `host:/x` names another machine and is none, and
    /// `src/app.rs:12` keeps its line. Named breaks: let a relative path
    /// take `=` -- `--out=build/x` reads whole and `--file=/tmp/a` loses
    /// `/tmp/a`; end it only at a space -- `a/b:/c/d` is none; start nothing
    /// after the list's `:` -- `a/b:/c/d` is `a/b` alone.
    #[test]
    fn a_relative_path_ends_before_an_equals_and_at_a_lists_colon() {
        let d = Forms::DEFAULT;
        assert_eq!(found("--file=/tmp/a", &d), ["/tmp/a"]);
        assert_eq!(found("--out=build/x", &d), ["build/x"]);
        assert_eq!(
            found("KEY=a/b.txt --config=foo.toml", &d),
            ["a/b.txt", "foo.toml"]
        );
        assert_eq!(found("a/b:/c/d", &d), ["a/b", "/c/d"]);
        assert_eq!(found("a/b:~/c", &d), ["a/b", "~/c"]);
        assert_eq!(found("host:/x example.com:/y", &d), Vec::<String>::new());
        let line_col = Forms {
            line_col: Turned::On,
            ..d
        };
        assert_eq!(
            found("src/app.rs:12 a/b:/c", &line_col),
            ["src/app.rs", "a/b", "/c"]
        );
    }

    /// **A Markdown link's or image's relative path is the path alone, its
    /// brackets left out**, as a README writes them: `![x](assets/baud.gif)`
    /// and `[t](docs/a.md)` are `assets/baud.gif` and `docs/a.md`, and an
    /// HTML `srcset="assets/readme/logo-dark.gif"` the quoted path; a
    /// relative path starts at a letter, a digit or one of `_.-@+`, never
    /// at the `!` or `[` before it, and ends at a closing bracket it did not
    /// open, whose wrapper's path the scan then reads; one with its own
    /// brackets keeps them. The prefixed forms are not changed. Named
    /// breaks: start a relative path anywhere -- the image reads as
    /// `![x](assets/baud.gif)`; end it only at a space -- the link reads as
    /// `t](docs/a.md`.
    #[test]
    fn a_markdown_links_relative_path_is_read_without_its_brackets() {
        let rel = Forms {
            bare: Turned::On,
            ..Forms::DEFAULT
        };
        assert_eq!(found("![x](assets/baud.gif)", &rel), ["assets/baud.gif"]);
        assert_eq!(found("see [t](docs/a.md).", &rel), ["docs/a.md"]);
        assert_eq!(
            found("<img srcset=\"assets/readme/logo-dark.gif\">", &rel),
            ["assets/readme/logo-dark.gif"]
        );
        assert_eq!(
            found("[guide](docs/guide/features.md) and [x](./a/b.md)", &rel),
            ["docs/guide/features.md", "./a/b.md"]
        );
        assert_eq!(found("src/f(1)/x.rs", &rel), ["src/f(1)/x.rs"], "its own");
        assert_eq!(
            found("@types/node .github/ci.yml", &rel),
            ["@types/node", ".github/ci.yml"]
        );
        assert_eq!(
            found("![x](/abs/a.gif) [t](~/b.md)", &Forms::DEFAULT),
            ["/abs/a.gif", "~/b.md"],
            "the prefixed forms, as before"
        );
    }

    /// **A path with a character it must not hold is refused whole**: a
    /// bidirectional control, a control, a marked cell. Perturbation: read a
    /// refused unit as the end of the path, keeping its first part.
    #[test]
    fn a_path_holding_a_control_or_a_mark_is_refused_whole() {
        let d = Forms::DEFAULT;
        assert_eq!(found("/tmp/\u{202E}txt.exe", &d), Vec::<String>::new());
        assert_eq!(found("/tmp/a\u{85}b", &d), Vec::<String>::new());
        let mut u = units("/tmp/ab");
        u[6] = Unit::Marked;
        let mut out = Vec::new();
        tokens(&u, (Edge::Line, Edge::Line), &d, &mut out);
        assert!(out.is_empty());
        assert_eq!(
            found("/tmp/caf\u{e9} ok", &d),
            ["/tmp/caf\u{e9}"],
            "the control: a letter is fine"
        );
    }

    /// **The scan's tail test is wide enough for every start it reads**: each
    /// start the text reads, its tail at the `/` says so, and with relative
    /// paths read, as by default, any `/` does. Perturbation: drop the `~` arm
    /// of `starts`.
    #[test]
    fn the_tail_test_admits_every_start() {
        let tail = |s: &str| {
            s.bytes()
                .fold(0u64, |a, b| (a << 8) | u64::from(b))
                .to_be_bytes()
        };
        for s in ["a/", ":/", "x../"] {
            assert!(starts(tail(s), &Forms::DEFAULT), "{s:?}");
        }
        let d = Forms {
            bare: Turned::Off,
            ..Forms::DEFAULT
        };
        for s in [" /", "(/", "=/", " ~/", " ./", " ../", "/"] {
            assert!(starts(tail(s), &d), "{s:?}");
        }
        for s in ["a/", ":/", "//", "x../"] {
            assert!(!starts(tail(s), &d), "{s:?}");
        }
        let no_parent = Forms {
            parent: Turned::Off,
            ..d
        };
        assert!(!starts(tail(" ../"), &no_parent));
    }

    /// **The URI is the path, percent-encoded, with no host**: unreserved
    /// characters and `/` as they are, every other byte as `%XX` in
    /// uppercase, UTF-8 first; and it is what `admit` reveals.
    /// Perturbation: keep a space; write lowercase hex.
    #[test]
    fn a_paths_uri_is_percent_encoded_utf8() {
        assert_eq!(file_uri(b"/tmp/a b"), "file:///tmp/a%20b");
        assert_eq!(
            file_uri("/tmp/caf\u{e9}".as_bytes()),
            "file:///tmp/caf%C3%A9"
        );
        assert_eq!(file_uri(b"/a?b#c%d"), "file:///a%3Fb%23c%25d");
        assert_eq!(file_uri(b"/x-y_z.~"), "file:///x-y_z.~");
        let uri = file_uri(b"/tmp/a b");
        let t = admit(uri.as_bytes(), &crate::link::MachineNames::NONE).expect("admitted");
        assert_eq!(t.action(), crate::link::Action::Reveal);
        let long = format!("/{}", "\u{e9}".repeat(60));
        assert!(!opens_as_uri(long.as_bytes()), "past LINK_MAX once encoded");
    }

    /// **A path gets a line only where the worker found it**: one found, one
    /// gone and one never answered, on the same row; the found one's cells
    /// are underlined, the others are not. Perturbation: draw a path with no
    /// answer; draw a missing one.
    #[test]
    fn a_path_gets_a_line_only_if_it_exists() {
        let (lines, p) = scanned(b"a /etc/hosts b /nope c ~/x", &["/etc/hosts"]);
        assert_eq!(lines, [(0, 2, 11)]);
        assert_eq!(p.result(b"/etc/hosts"), Some(Probe::Exists));
        assert_eq!(p.result(b"/nope"), Some(Probe::Missing));
        assert_eq!(p.result(b"/home/user/x"), Some(Probe::Missing), "~ is HOME");
        assert_eq!(p.result(b"/elsewhere"), None, "never asked");
    }

    /// **A frame asks nothing; a collection asks for each path the screen
    /// holds once, and drops the answers it no longer shows.** The scan of a
    /// frame only notes that a row changed. Perturbation: push to the
    /// queue outside a collection; keep the old answers in `end_collect`.
    #[test]
    fn a_new_screen_replaces_the_queue() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"/a /a /b\r\n");
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::On,
                paths: &mut p,
            },
        );
        assert_eq!(p.owed(), Owed::Yes, "a frame owes");
        p.begin_collect();
        l.forget();
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::On,
                paths: &mut p,
            },
        );
        let asked: Vec<Vec<u8>> = p.end_collect().into_iter().map(|c| c.path).collect();
        assert_eq!(asked, [b"/a".to_vec(), b"/b".to_vec()], "each once");
        assert_eq!(p.owed(), Owed::No);
        let _ = p.merge([
            (b"/a".to_vec(), Probe::Exists),
            (b"/b".to_vec(), Probe::Exists),
        ]);
        // A new screen: /b gone, /c new.
        feed(&mut t, b"\x1b[2J\x1b[H/a /c\r\n");
        p.begin_collect();
        l.forget();
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::On,
                paths: &mut p,
            },
        );
        let asked: Vec<Vec<u8>> = p.end_collect().into_iter().map(|c| c.path).collect();
        assert_eq!(asked, [b"/c".to_vec()], "only what has no answer");
        assert_eq!(p.result(b"/a"), Some(Probe::Exists), "kept: still shown");
        assert_eq!(p.result(b"/b"), None, "off the screen: forgotten");
    }

    /// **A batch moves the generation once, and an answer that changes
    /// nothing moves nothing**, so the lines are read again once a batch.
    /// Perturbation: move the generation per answer, or on an equal one.
    #[test]
    fn a_batch_moves_the_generation_once() {
        let (_, mut p) = scanned(b"/a /b", &[]);
        let g = p.generation();
        assert_eq!(
            p.merge([
                (b"/a".to_vec(), Probe::Exists),
                (b"/b".to_vec(), Probe::Exists)
            ]),
            Moved::Yes
        );
        assert_eq!(p.generation(), g.next());
        assert_eq!(p.merge([(b"/a".to_vec(), Probe::Exists)]), Moved::No);
        assert_eq!(
            p.merge([(b"/zz".to_vec(), Probe::Exists)]),
            Moved::No,
            "not on screen"
        );
        assert_eq!(p.generation(), g.next());
    }

    /// **`./` is read only against a report that names this machine, on the
    /// rows from where it took effect**: with no names, with `localhost` and
    /// with another host, none; with this machine's, the rows at and below the
    /// cursor's line when it came, and none above. Perturbation: accept
    /// `FileHost::Local`; drop the `from` test.
    #[test]
    fn dot_needs_a_report_naming_this_machine() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"./early\r\n\x1b]7;file://here.example/w\x07");
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        let _ = p.observe(&t, at());
        feed(&mut t, b"./late\r\n");
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let mut ask = |p: &mut Paths, t: &Term<'_>| {
            p.begin_collect();
            l.forget();
            let _ = l.scan(
                t,
                at(),
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: p,
                },
            );
            p.end_collect()
                .into_iter()
                .map(|c| c.path)
                .collect::<Vec<_>>()
        };
        assert!(ask(&mut p, &t).is_empty(), "no names: the host is unknown");
        p.set_names(crate::link::MachineNames::of(vec![
            "here.example".to_string(),
        ]));
        assert_eq!(
            ask(&mut p, &t),
            [b"/w/late".to_vec()],
            "the row above the report gets none"
        );
        feed(&mut t, b"\x1b]7;file://localhost/v\x07./again\r\n");
        let _ = p.observe(&t, at());
        assert!(ask(&mut p, &t).is_empty(), "localhost names no machine");
        feed(&mut t, b"\x1b]7;file://other.example/v\x07./again\r\n");
        let _ = p.observe(&t, at());
        assert!(ask(&mut p, &t).is_empty(), "another machine's");
    }

    /// **Rows a program not known to be local wrote get no mark, after it
    /// ends too, and rows a multiplexer wrote take only `./` away.** The row
    /// filed as the worker files the groups seen at its reads. Named breaks:
    /// drop the span when the front comes back local; read `./` on a row a
    /// multiplexer wrote.
    #[test]
    fn rows_from_ssh_keep_no_mark_after_it_ends() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b]7;file://here.example/w\x07");
        let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        let fresh = || {
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            p.all_written_here();
            p.set_names(crate::link::MachineNames::of(vec![
                "here.example".to_string(),
            ]));
            p
        };
        let mut p = fresh();
        let _ = p.observe(&t, at());
        feed(&mut t, b"/etc/hosts ./x\r\n");
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let mut ask = |p: &mut Paths| {
            p.begin_collect();
            l.forget();
            let _ = l.scan(
                &t,
                at(),
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: p,
                },
            );
            p.end_collect()
                .into_iter()
                .map(|c| c.path)
                .collect::<Vec<_>>()
        };
        assert_eq!(ask(&mut p).len(), 2, "the control: both");
        p.written_under(Seen::Named(Front::Multiplexer), LineSpan::of(first, first));
        p.set_front(Front::Multiplexer, on_primary(first.below()));
        assert_eq!(
            ask(&mut p),
            [b"/etc/hosts".to_vec()],
            "a multiplexer: no ./"
        );
        let mut p = fresh();
        let _ = p.observe(&t, at());
        p.written_under(Seen::Named(Front::Elsewhere), LineSpan::of(first, first));
        p.set_front(Front::Elsewhere, on_primary(first.below()));
        assert!(ask(&mut p).is_empty(), "ssh in front: nothing");
        p.set_front(Front::Local, on_primary(first.below()));
        assert!(ask(&mut p).is_empty(), "and its rows stay unmarked after");
    }

    /// **A frame of output with no path kept reads no row for paths**: its
    /// changed rows owe a batch, and only the collection reads them. Then,
    /// with a path found, a frame's changed rows are read, to draw it.
    /// Perturbation: read paths on every frame in `Links::read`, or let
    /// `changed` not owe.
    #[test]
    fn a_frame_reads_paths_only_once_one_is_drawn() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"/// a comment\r\nx /etc/hosts\r\n");
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let mut scan = |p: &mut Paths, l: &mut Lines| {
            l.scan(
                &t,
                at(),
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: p,
                },
            )
        };
        assert_eq!(scan(&mut p, &mut l).candidates, 0, "a frame reads none");
        assert_eq!(p.owed(), Owed::Yes, "but owes a batch");
        p.begin_collect();
        l.forget();
        assert_eq!(
            scan(&mut p, &mut l).candidates,
            1,
            "the control: a collection reads it"
        );
        let asked = p.end_collect();
        let _ = p.merge(asked.into_iter().map(|c| (c.path, Probe::Exists)));
        l.forget();
        assert_eq!(
            scan(&mut p, &mut l).candidates,
            1,
            "one found: a frame reads"
        );
        assert_eq!(runs(&l), [(1, 2, 11)]);
    }

    /// A window of `bytes`, its paths on, and a frame's scan of it.
    struct Pended<'s> {
        t: Term<'s>,
        p: Paths,
        l: Lines,
        u: Uris,
    }

    impl Pended<'_> {
        /// A frame's scan.
        fn frame(&mut self) {
            let _ = self.l.scan(
                &self.t,
                at(),
                &mut Links {
                    uris: &mut self.u,
                    urls: Turned::On,
                    paths: &mut self.p,
                },
            );
        }

        /// A collection's scan, every row read again: what it asked.
        fn collect(&mut self) -> Vec<Vec<u8>> {
            self.p.begin_collect();
            self.l.forget();
            self.frame();
            self.p.end_collect().into_iter().map(|c| c.path).collect()
        }

        /// `row` of the window, as one line's span.
        fn row(y: baud_domain::Row) -> LineSpan {
            let line = at().line(Rows::ZERO, y);
            LineSpan::of(line, line)
        }

        /// `rows` named, written here, and no longer still to be named;
        /// every row read again, as the driver does for an answer that
        /// moved what is in front (`probe::Probes::answers`).
        fn name(&mut self, rows: &[baud_domain::Row]) {
            for &y in rows {
                self.p
                    .written_under(Seen::Named(Front::Local), Self::row(y));
            }
            self.p.set_pending(core::iter::empty(), core::iter::empty());
            self.l.forget();
        }
    }

    /// **A path on a row still to be named owes its naming for a path, and
    /// the batch after the naming for it too**: `/etc/hosts` written on a
    /// row whose writer is still to be named, no answer kept anywhere. A
    /// frame reads that row alone ([`Reads::Pending`]) and owes a batch
    /// for a path; the naming batch's collection asks nothing, the row not
    /// readable, and notes the path ([`Awaiting::Unanswered`]); the answer
    /// naming the row owes the next batch for a path, which asks about it.
    /// Once the path is owed, no line is read for paths until a batch goes.
    /// The control: `% `, no path, owes for the rows alone, and nothing
    /// after its naming, its lines still read. Breaks: no [`Reads::Pending`]
    /// -- the frame owes for the rows; the naming owing nothing after it --
    /// nothing owed once the row is named; lines read for paths whatever is
    /// owed -- a line still wanted once the path is owed.
    #[test]
    fn a_pending_rows_path_awaits_its_naming() {
        let run = |bytes: &[u8]| {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            feed(&mut t, bytes);
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            let _ = p.observe(&t, at());
            p.set_pending(
                [Pended::row(baud_domain::Row::from_index(0))].into_iter(),
                core::iter::empty(),
            );
            let mut w = Pended {
                t,
                p,
                l: Lines::default(),
                u: Uris::default(),
            };
            let before = w.p.wants(u64::from(b'/').to_be_bytes());
            w.frame();
            let frame = w.p.owes();
            let wants = (before, w.p.wants(u64::from(b'/').to_be_bytes()));
            let naming = w.collect();
            w.name(&[baud_domain::Row::ZERO]);
            let after = w.p.owes();
            let asked = w.collect();
            (frame, wants, naming, after, asked)
        };
        let (frame, wants, naming, after, asked) = run(b"/etc/hosts\r\n");
        assert_eq!(frame, Some(Owes::Path), "a frame owes the naming");
        assert_eq!(wants, (true, false), "no line read for paths once owed");
        assert_eq!(naming, Vec::<Vec<u8>>::new(), "the naming asks nothing");
        assert_eq!(after, Some(Owes::Path), "and owes the batch after it");
        assert_eq!(asked, [b"/etc/hosts".to_vec()], "which asks");
        let (frame, wants, _, after, asked) = run(b"% \r\n");
        assert_eq!(wants, (true, true), "the control: read while a path may be");
        assert_eq!(frame, Some(Owes::Rows), "the control: no path");
        assert_eq!(after, None, "the control: nothing after its naming");
        assert_eq!(asked, Vec::<Vec<u8>>::new());
    }

    /// **A path on a row still to be named whose answer is kept owes the
    /// naming, and nothing after it**: `/etc/hosts` asked and found on row
    /// 0, then printed again on row 1, whose writer is still to be named.
    /// A frame owes a batch for a path; the naming batch's collection asks
    /// nothing and notes the answer kept ([`Awaiting::Cached`]); once the
    /// row is named nothing more is owed, and the next frame draws both
    /// rows' marks from the kept answer. Break: a kept answer awaited as
    /// none is -- a path owed after the naming.
    #[test]
    fn a_cached_one_awaits_nothing_once_named() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"/etc/hosts\r\n");
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        let _ = p.observe(&t, at());
        p.written_under(
            Seen::Named(Front::Local),
            Pended::row(baud_domain::Row::from_index(0)),
        );
        let mut w = Pended {
            t,
            p,
            l: Lines::default(),
            u: Uris::default(),
        };
        let asked = w.collect();
        assert_eq!(asked, [b"/etc/hosts".to_vec()], "the control");
        let _ =
            w.p.merge_into(asked.into_iter().map(|a| (a, Probe::Exists)), &mut w.l);
        feed(&mut w.t, b"/etc/hosts\r\n");
        w.p.set_pending(
            [Pended::row(baud_domain::Row::from_index(1))].into_iter(),
            core::iter::empty(),
        );
        w.frame();
        assert_eq!(w.p.owes(), Some(Owes::Path), "a frame owes the naming");
        assert_eq!(w.collect(), Vec::<Vec<u8>>::new(), "nothing asked");
        w.name(&[baud_domain::Row::from_index(1)]);
        assert_eq!(w.p.owes(), None, "nothing after the naming");
        w.frame();
        assert_eq!(runs(&w.l), [(0, 0, 9), (1, 0, 9)], "both drawn");
    }

    /// **A row a frame read while only rows still to be named were read
    /// is read again once an answer is drawn**: `/etc/hosts` on row 0,
    /// asked by a collection, with row 5's writer still to be named; then
    /// `/etc/hosts` again on row 1, written here, read by a frame under
    /// [`Reads::Pending`], which keeps no run for it; then the answer,
    /// found. Both rows draw it. Break: [`Reads::Pending`] counted as
    /// reading paths for [`Unread`] -- row 1 is not read again, and stays
    /// unmarked.
    #[test]
    fn a_row_read_while_only_pending_rows_are_read_is_read_again() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"/etc/hosts\r\n");
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        let _ = p.observe(&t, at());
        p.all_written_here();
        p.set_pending(
            [Pended::row(baud_domain::Row::from_index(5))].into_iter(),
            core::iter::empty(),
        );
        let mut w = Pended {
            t,
            p,
            l: Lines::default(),
            u: Uris::default(),
        };
        let asked = w.collect();
        assert_eq!(asked, [b"/etc/hosts".to_vec()], "the control");
        feed(&mut w.t, b"/etc/hosts\r\n");
        w.frame();
        let _ =
            w.p.merge_into(asked.into_iter().map(|a| (a, Probe::Exists)), &mut w.l);
        w.frame();
        assert_eq!(runs(&w.l), [(0, 0, 9), (1, 0, 9)], "both drawn");
    }

    /// **`paths = "off"` reads and asks nothing**, and drops what it had; a
    /// closed window stays closed. Perturbation: keep the answers when
    /// turned off; let `set` reopen a closed window.
    #[test]
    fn paths_off_reads_nothing_and_closed_stays_closed() {
        let (lines, mut p) = scanned(b"/etc/hosts", &["/etc/hosts"]);
        assert_eq!(lines, [(0, 0, 9)], "the control");
        p.set(Turned::Off);
        assert_eq!(p.result(b"/etc/hosts"), None);
        assert!(!p.wants(u64::from(b'/').to_be_bytes()));
        p.set(Turned::On);
        p.all_written_here();
        p.close();
        p.set(Turned::On);
        assert_eq!(p.marks(), Marks::Closed);
        assert!(!p.wants(u64::from(b'/').to_be_bytes()));
    }

    /// **A preview or a click reads the path the scan drew, and only one the
    /// worker found or found gone.** Perturbation: answer a path with no
    /// answer in `PathsAt::at`.
    #[test]
    fn a_click_reads_the_path_the_line_drew() {
        let (_, p) = scanned(b"x /etc/hosts /nope /new", &["/etc/hosts"]);
        let u = units("x /etc/hosts /nope /new");
        let line = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        let read = |i| {
            p.at(
                &u,
                Ix(i),
                (Edge::Line, Edge::Line),
                (line, line),
                Screen::Normal,
            )
        };
        let hosts = read(5).expect("found");
        assert_eq!((hosts.span.clone(), hosts.probe), (2..12, Probe::Exists));
        assert_eq!(read(15).expect("gone").probe, Probe::Missing);
        assert_eq!(read(0), None, "no path there");
        let mut q = Paths::new(None, Forms::DEFAULT);
        q.set(Turned::On);
        q.all_written_here();
        assert_eq!(
            q.at(
                &u,
                Ix(5),
                (Edge::Line, Edge::Line),
                (line, line),
                Screen::Normal
            ),
            None,
            "never answered"
        );
    }

    /// **A path the scan draws opens**: every run the scan gives a path is the
    /// cells of one whose URI `admit` takes. Perturbation: drop
    /// `opens_as_uri` from `read`.
    #[test]
    fn a_path_line_is_what_a_click_admits() {
        let long = format!("/{}", "\u{e9}".repeat(43));
        assert!(
            file_uri(long.as_bytes()).len() > MAX,
            "the control: past the cap encoded"
        );
        let text = format!("{long} /ok");
        let (lines, p) = scanned(text.as_bytes(), &[long.as_str(), "/ok"]);
        assert_eq!(
            p.result(long.as_bytes()),
            Some(Probe::Exists),
            "the control: it was found"
        );
        assert_eq!(lines, [(0, 45, 47)], "the long one gets none");
    }

    /// **A path the worker found is a link the click and the preview read,
    /// and the bar names this machine**; one found gone is refused in words,
    /// with no line; with `urls = "off"` the look at rest is on paths only.
    /// The press and its release over the same path open it. Perturbations:
    /// drop the machine's label in `preview_text`; let `printed_in` admit a
    /// missing path; let `PathsOnly` paint a URI.
    #[test]
    fn a_found_path_opens_and_its_bar_names_this_machine() {
        use crate::link::{
            AtRest, Chord, Clicks, Gesture, Hover, Press, Spot, Under, gesture, preview_text,
            under_with,
        };
        use baud_core::Focus;
        use baud_domain::{Col, Row};
        let text = b"x /etc/hosts /nope https://a.example/";
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, text);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        let (mut l, mut u) = (Lines::default(), Uris::default());
        p.begin_collect();
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::On,
                paths: &mut p,
            },
        );
        let asked = p.end_collect();
        let _ = p.merge(asked.into_iter().map(|c| {
            let probe = match c.path.as_slice() {
                b"/etc/hosts" => Probe::Exists,
                _ => Probe::Missing,
            };
            (c.path, probe)
        }));
        let spot = |col| Spot {
            scrolled: at(),
            back: Rows::ZERO,
            row: Row::ZERO,
            col: Col::at_index(col),
        };
        let read = |col| {
            under_with(
                &t,
                spot(col),
                Press::Down,
                Chord::Cmd,
                Focus::Gained,
                None,
                &p,
            )
        };
        let found = read(5).expect("a link");
        let Under::Path(printed) = &found else {
            panic!("a printed path: {found:?}");
        };
        assert_eq!(printed.uri(), "file:///etc/hosts");
        let bar = preview_text(
            &found,
            &t,
            &crate::link::MachineNames::NONE,
            Cols::of_index(58),
        )
        .expect("a bar");
        assert!(bar.starts_with(&format!("On {MACHINE}")), "{bar}");
        assert!(bar.ends_with("file:///etc/hosts"), "{bar}");
        let gone = read(15).expect("a refused link");
        let bar = preview_text(
            &gone,
            &t,
            &crate::link::MachineNames::NONE,
            Cols::of_index(58),
        )
        .expect("a bar");
        assert!(bar.contains("no such file on"), "{bar}");
        assert_eq!(
            under_with(
                &t,
                spot(5),
                Press::Down,
                Chord::Cmd,
                Focus::Gained,
                None,
                &NoPaths
            ),
            None,
            "the control: no paths, no link"
        );
        // The press and the release over the same path open it.
        let g = gesture(
            Press::Down,
            Chord::Cmd,
            Clicks::Single,
            Focus::Gained,
            None,
            Some(&found),
        );
        let Gesture::Arm(armed) = g else {
            panic!("{g:?}")
        };
        let up = under_with(
            &t,
            spot(7),
            Press::Up,
            Chord::Other,
            Focus::Gained,
            Some(&armed),
            &p,
        );
        assert_eq!(
            gesture(
                Press::Up,
                Chord::Other,
                Clicks::Single,
                Focus::Gained,
                Some(&armed),
                up.as_ref()
            ),
            Gesture::Open(found.clone())
        );
        // `urls = "off"`: the look on the path, none on the URI.
        let mut h = Hover::default();
        h.set_at_rest(AtRest::PathsOnly(baud_render::HoverPaint::Box));
        let rest = |h: &mut Hover, col| {
            let _ = h.update_with(
                &t,
                Some((Row::ZERO, Col::at_index(col))),
                at(),
                Chord::Other,
                Focus::Gained,
                &crate::link::MachineNames::NONE,
                &p,
            );
            h.over().is_some()
        };
        assert!(rest(&mut h, 5), "the path");
        assert!(!rest(&mut h, 25), "the URI, with urls off");
        assert!(!rest(&mut h, 15), "the missing path");
    }

    /// **`urls = "off"` with paths on reads paths and no URI.** Perturbation:
    /// read URIs in `Links::read` whatever `urls` says.
    #[test]
    fn urls_off_reads_paths_only() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"/etc/hosts https://a.example/");
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        let (mut l, mut u) = (Lines::default(), Uris::default());
        p.begin_collect();
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::Off,
                paths: &mut p,
            },
        );
        let asked = p.end_collect();
        let _ = p.merge(asked.into_iter().map(|c| (c.path, Probe::Exists)));
        l.forget();
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::Off,
                paths: &mut p,
            },
        );
        assert_eq!(runs(&l), [(0, 0, 9)], "the path, not the URI");
    }

    /// **No program can read a path's mark back**: `DECRQCRA` answers the
    /// same before the scan and with the path found and underlined, since the
    /// line is the frame's and never a cell's. The control: a cell written
    /// moves the answer, so the checksum sees that row. Perturbation: write
    /// the mark into the cells the scan reads.
    #[test]
    fn decrqcra_answers_the_same_with_a_path_marked() {
        let checksum = |t: &mut Term<'_>| {
            t.clear_reply();
            feed(t, b"\x1b[7;1;1;1;6;60*y");
            t.reply().to_vec()
        };
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?25l/etc/hosts");
        let before = checksum(&mut t);
        assert!(!before.is_empty(), "the control: the query answers");
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        let (mut l, mut u) = (Lines::default(), Uris::default());
        p.begin_collect();
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::On,
                paths: &mut p,
            },
        );
        let asked = p.end_collect();
        let _ = p.merge(asked.into_iter().map(|c| (c.path, Probe::Exists)));
        l.forget();
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::On,
                paths: &mut p,
            },
        );
        assert_eq!(runs(&l), [(0, 0, 9)], "the control: it is marked");
        assert_eq!(checksum(&mut t), before, "the same with the mark up");
        feed(&mut t, b"\x1b[1;1HX");
        assert_ne!(checksum(&mut t), before, "the control: a cell moves it");
    }

    /// This machine's name in the strike's tests.
    const HERE: &str = "here.example";

    fn struck(l: &Lines) -> Vec<(usize, usize, usize)> {
        l.struck()
            .iter()
            .map(|s| (s.row.index(), s.from.index(), s.to.index()))
            .collect()
    }

    /// An `OSC 8` link to `uri` over `text`.
    fn marked(uri: &str, text: &str) -> String {
        format!("\x1b]8;;{uri}\x1b\\{text}\x1b]8;;\x1b\\")
    }

    /// **A window's links with this machine named**, `set` given the paths
    /// before the scan, the worker's answer for each path asked `answer`'s:
    /// the lines, the struck runs, the paths asked about and the paths.
    #[allow(clippy::type_complexity)]
    fn struck_scan(
        bytes: &[u8],
        answer: &dyn Fn(&[u8]) -> Probe,
        set: &dyn Fn(&mut Paths),
    ) -> (
        Vec<(usize, usize, usize)>,
        Vec<(usize, usize, usize)>,
        Vec<Vec<u8>>,
        Paths,
    ) {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, bytes);
        let mut p = Paths::new(Some(b"/home/user".to_vec()), Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
        set(&mut p);
        let mut u = Uris::default();
        u.set_names(crate::link::MachineNames::one(HERE));
        let mut l = Lines::default();
        let _ = p.observe(&t, at());
        p.begin_collect();
        let mut links = |p: &mut Paths, l: &mut Lines| {
            let _ = l.scan(
                &t,
                at(),
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: p,
                },
            );
        };
        links(&mut p, &mut l);
        let asked: Vec<Vec<u8>> = p.end_collect().into_iter().map(|c| c.path).collect();
        let answers = asked
            .iter()
            .map(|a| (a.clone(), answer(a)))
            .collect::<Vec<_>>();
        let _ = p.merge_into(answers, &mut l);
        links(&mut p, &mut l);
        (runs(&l), struck(&l), asked, p)
    }

    /// **A batch of answers redraws the printed paths with no row read,
    /// once paths are read**: the first answer that finds one starts the
    /// reading of paths, and every row is read again, the row printed since
    /// the collection among them; later answers draw and take away runs
    /// with no row read, each as a scan from nothing draws. Named breaks:
    /// read every row again on any answer (`merge_into` forgetting) -- a
    /// later answer reads the rows; redraw on the answer that starts the
    /// reading -- the row printed since is not drawn.
    #[test]
    fn an_answer_redraws_printed_paths_with_no_row_read() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"/etc/hosts /etc/passwd\r\n");
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let _ = p.observe(&t, at());
        let asked = collect(&t, &mut p, &mut l, &mut u);
        assert_eq!(paths_of(&asked), ["/etc/hosts", "/etc/passwd"]);
        feed(&mut t, b"/etc/hosts again\r\n");
        frame(&t, &mut p, &mut l, &mut u);
        let mut scan = |p: &mut Paths, l: &mut Lines| {
            l.scan(
                &t,
                at(),
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: p,
                },
            )
        };
        let fresh = |p: &mut Paths| {
            let mut l = Lines::default();
            let _ = l.scan(
                &t,
                at(),
                &mut Links {
                    uris: &mut Uris::default(),
                    urls: Turned::On,
                    paths: p,
                },
            );
            runs(&l)
        };
        let answer = |path: &str, probe| [(path.as_bytes().to_vec(), probe)];
        let _ = p.merge_into(
            [
                (b"/etc/hosts".to_vec(), Probe::Exists),
                (b"/etc/passwd".to_vec(), Probe::Missing),
            ],
            &mut l,
        );
        let s = scan(&mut p, &mut l);
        assert!(s.rows >= 2, "reads start: every row read again: {s:?}");
        assert_eq!(
            runs(&l),
            [(0, 0, 9), (1, 0, 9)],
            "the row printed since too"
        );
        assert_eq!(runs(&l), fresh(&mut p));
        for (path, probe, lit, remarks) in [
            (
                "/etc/passwd",
                Probe::Exists,
                &[(0, 0, 9), (0, 11, 21), (1, 0, 9)][..],
                1,
            ),
            ("/etc/hosts", Probe::Missing, &[(0, 11, 21)], 2),
        ] {
            assert_eq!(p.merge_into(answer(path, probe), &mut l), Moved::Yes);
            let s = scan(&mut p, &mut l);
            assert_eq!((s.rows, s.remarks, s.rereads), (0, remarks, 0), "{path}");
            assert_eq!(runs(&l), lit, "{path}");
            assert_eq!(runs(&l), fresh(&mut p), "{path}");
        }
        assert_eq!(
            p.merge_into(answer("/etc/hosts", Probe::Missing), &mut l),
            Moved::No,
            "the same answer again"
        );
        assert_eq!(
            scan(&mut p, &mut l),
            crate::lined::Scan::default(),
            "and nothing done"
        );
    }

    /// **An `OSC 8` `file://` link naming this machine, its file gone, is
    /// struck at rest, a struck run and not a live line**; one found is a
    /// live line; and with `missing = "off"` the gone one is a live line as
    /// before and nothing is asked for it. Perturbations: strike on `Named::NoMachine` in
    /// `file_link`; let `file_link` ignore `missing`; push a struck run into
    /// the lines in `osc8`.
    #[test]
    fn a_marked_file_here_and_gone_is_struck_at_rest() {
        let text = format!(
            "{} {}",
            marked("file://here.example/gone", "gone"),
            marked("file://here.example/etc/hosts", "hosts")
        );
        let answer = |p: &[u8]| match p {
            b"/etc/hosts" => Probe::Exists,
            _ => Probe::Missing,
        };
        let (lines, strikes, asked, p) = struck_scan(text.as_bytes(), &answer, &|_| {});
        assert_eq!(asked, [b"/gone".to_vec(), b"/etc/hosts".to_vec()]);
        assert_eq!(strikes, [(0, 0, 3)], "gone: struck");
        assert_eq!(
            lines,
            [(0, 5, 9)],
            "found: underlined, and the gone one not"
        );
        assert_eq!(p.result(b"/gone"), Some(Probe::Missing));
        let (lines, strikes, asked, _) = struck_scan(text.as_bytes(), &answer, &|p| {
            let _ = p.set_missing(Turned::Off);
        });
        assert!(strikes.is_empty(), "missing off: nothing struck");
        assert!(
            asked.is_empty(),
            "missing off: nothing asked for a marked link"
        );
        assert_eq!(lines, [(0, 0, 3), (0, 5, 9)], "both underlined as before");
    }

    /// What a click on a struck link came to, the re-check answering
    /// `again`, as both drivers carry it out: `link::click`, then for a
    /// link asked again `paths::on_found` with `Asked::Link`.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Came {
        Copy,
        Command,
        Window,
        Open,
        Reveal,
        PathCopy,
        PathCommand,
        PathWindow,
        SaidGone,
    }

    fn click_came(t: &Target<'_>, p: &Paths, again: Probe, rules: &Rules) -> Came {
        use crate::link::{Click, click};
        match click(t, p) {
            Click::Copy => Came::Copy,
            Click::Command => Came::Command,
            Click::Window => Came::Window,
            Click::Open => Came::Open,
            Click::Reveal => Came::Reveal,
            Click::Again(path) => match again {
                Probe::Exists => match on_found(
                    &Asked::Link,
                    &path,
                    &crate::link::MachineNames::one(HERE),
                    rules,
                ) {
                    Found::Copy => Came::PathCopy,
                    Found::Command(_) => Came::PathCommand,
                    Found::Window(_) => Came::PathWindow,
                    Found::Reveal => Came::Reveal,
                },
                _ => Came::SaidGone,
            },
        }
    }

    /// **A click on a struck `file://` link runs the link's own action, on
    /// both drivers, in four cases**: a `[link.file]` copy rule while its
    /// file is missing and once it reappears, and a command rule the same,
    /// each with a `[link.path]` command set, which a reappeared link never
    /// takes. Both drivers dispatch on `link::click` and nothing else, the
    /// re-check's answer on `paths::on_found`, so one table holds the macOS
    /// and the Linux click alike; the control is the same link with no rule,
    /// which is asked again and said gone, or revealed. Named break: in
    /// `click`, ask `paths.gone` before reading the action, the order the
    /// Linux driver had, and all four cases are asked again instead.
    #[test]
    fn a_struck_file_links_rule_runs_on_both_drivers_missing_or_back() {
        let text = marked("file://here.example/gone", "gone");
        let (_, strikes, _, p) = struck_scan(text.as_bytes(), &|_| Probe::Missing, &|_| {});
        assert_eq!(strikes, [(0, 0, 3)], "the control: struck, found gone");
        let uri = b"file://here.example/gone";
        let echo = tool("echo");
        let path = format!("[link.path]\ncommand = [\"{echo}\", \"{{path}}\"]\n");
        let rule = |r: &str| {
            let mut c = crate::config::Config::defaults();
            let said = c.apply(&format!("{path}{r}"));
            assert!(said.is_empty(), "{r}: {said:?}");
            c.link.rules
        };
        let copy = rule("[link.file]\naction = \"copy\"\n");
        let command = rule(&format!(
            "[link.file]\ncommand = [\"{echo}\", \"{{url}}\"]\n"
        ));
        let none = rule("");
        let mut came = Vec::new();
        for (name, rules) in [("copy", &copy), ("command", &command)] {
            let t = crate::link::admit_link(
                uri,
                &crate::link::MachineNames::one(HERE),
                rules,
                LinkOrigin::given(Origin::Local),
            )
            .expect("the rule's");
            for (state, again) in [("missing", Probe::Missing), ("back", Probe::Exists)] {
                came.push((name, state, click_came(&t, &p, again, rules)));
            }
        }
        assert_eq!(
            came,
            [
                ("copy", "missing", Came::Copy),
                ("copy", "back", Came::Copy),
                ("command", "missing", Came::Command),
                ("command", "back", Came::Command),
            ],
            "each case runs its rule"
        );
        let plain = crate::link::admit_link(
            uri,
            &crate::link::MachineNames::one(HERE),
            &none,
            LinkOrigin::given(Origin::Local),
        )
        .expect("admitted");
        assert_eq!(
            click_came(&plain, &p, Probe::Missing, &none),
            Came::SaidGone
        );
        assert_eq!(click_came(&plain, &p, Probe::Exists, &none), Came::Reveal);
    }

    /// Every link the worker says is gone, whatever it is: what reaches
    /// `click`'s asking for a link of any action.
    struct AllGone;

    impl PathsAt for AllGone {
        fn generation(&self) -> Generation {
            Generation::default()
        }

        fn printed(&self, term: &Term<'_>, link: &crate::link::Under) -> crate::link::Printed {
            NoPaths.printed(term, link)
        }

        fn at(
            &self,
            _: &[Unit],
            _: Ix,
            _: (Edge, Edge),
            _: (AbsLine, AbsLine),
            _: Screen,
        ) -> Option<PathAt> {
            None
        }

        fn gone(&self, _: &Target<'_>) -> Option<Gone> {
            Some(Gone::File(b"/gone".to_vec()))
        }
    }

    /// **No path opens a found file with the system's app**: a link that
    /// opens is never asked about again, even where its file is said gone,
    /// only one that reveals is; and an ask, found, is a reveal or a
    /// printed path's `[link.path]` action, never an open. Named break: in
    /// `click`, ask about an open link's file again too, as the Open arm
    /// did, and the `https` link is asked again.
    #[test]
    fn no_path_opens_a_found_file() {
        use crate::link::{Click, click};
        let opens =
            admit(b"https://a.example/", &crate::link::MachineNames::NONE).expect("admitted");
        assert_eq!(opens.action(), crate::link::Action::Open);
        assert_eq!(
            click(&opens, &AllGone),
            Click::Open,
            "an open is never asked again"
        );
        let reveals = admit(b"file:///gone", &crate::link::MachineNames::NONE).expect("admitted");
        assert_eq!(
            click(&reveals, &AllGone),
            Click::Again(b"/gone".to_vec()),
            "the control: a reveal is"
        );
        let rules = &Rules::NONE;
        for asked in [Asked::Link, Asked::Printed(LineCol::default())] {
            let found = on_found(
                &asked,
                b"/etc/hosts",
                &crate::link::MachineNames::NONE,
                rules,
            );
            assert!(matches!(found, Found::Reveal), "{asked:?}: {found:?}");
        }
    }

    /// **Two clicks before one answer: each answer finds its own ask**. A
    /// gone link clicked, then a printed path before the worker answers:
    /// the printed path's answer takes its own `:line:col`, and an answer
    /// for the link's path -- the earlier ask, which the worker drops, so
    /// only a stale one could come -- is a link's reveal and never the
    /// printed path's `[link.path]` action. Named break: in `asked_for`, take
    /// an answer that matches no ask as a printed path with no `:line:col`,
    /// as both drivers did, and the link's answer gets `[link.path]`'s.
    #[test]
    fn two_clicks_before_one_answer_each_get_their_own_action() {
        let lc = LineCol {
            line: "12".to_string(),
            col: "3".to_string(),
        };
        let mut slot = Some((b"/gone".to_vec(), Asked::Link));
        let earlier = slot.replace((b"/x".to_vec(), Asked::Printed(lc.clone())));
        assert_eq!(
            earlier.map(|(p, _)| p),
            Some(b"/gone".to_vec()),
            "the second click takes the slot, as both drivers keep it"
        );
        assert_eq!(
            asked_for(slot.clone(), b"/x"),
            Asked::Printed(lc),
            "its own"
        );
        assert_eq!(
            asked_for(slot, b"/gone"),
            Asked::Link,
            "the link's answer is a link's"
        );
        let slot = Some((b"/gone".to_vec(), Asked::Link));
        assert_eq!(asked_for(slot, b"/gone"), Asked::Link, "its own");
    }

    /// **A link asked again, its file found, is revealed, never handed to
    /// `[link.path]`'s command**, which a printed path asked again is: both
    /// drivers decide through `on_found`. Named break: in `on_found`, read a link
    /// as a printed path with no `:line:col`, and the link gets the command.
    #[test]
    fn a_gone_then_found_link_reveals_whatever_link_path_says() {
        let mut c = crate::config::Config::defaults();
        let said = c.apply(&format!(
            "[link.path]\ncommand = [\"{}\", \"{{path}}\"]\n",
            tool("echo")
        ));
        assert!(said.is_empty(), "{said:?}");
        let rules = &c.link.rules;
        let printed = on_found(
            &Asked::Printed(LineCol::default()),
            b"/etc/hosts",
            &crate::link::MachineNames::NONE,
            rules,
        );
        assert!(
            matches!(printed, Found::Command(Ok(_))),
            "the control: a printed path runs [link.path]'s command: {printed:?}"
        );
        let link = on_found(
            &Asked::Link,
            b"/etc/hosts",
            &crate::link::MachineNames::NONE,
            rules,
        );
        assert!(matches!(link, Found::Reveal), "a link reveals: {link:?}");
    }

    /// **A printed path asked again under `[link.path] action = "window"`,
    /// its file found, comes to the window's argv**, filled with the path
    /// and its `:line:col`, and a link asked again still reveals; neither
    /// runs a command. Named break: in `on_found`, take a window rule as no
    /// command at all -- the path is revealed.
    #[test]
    fn a_found_printed_path_comes_to_its_window() {
        let mut c = crate::config::Config::defaults();
        let echo = tool("echo");
        let said = c.apply(&format!(
            "[link.path]\nline_col = \"on\"\naction = \"window\"\n\
             command = [\"{echo}\", \"{{path}}:{{line}}\"]\n",
        ));
        assert!(said.is_empty(), "{said:?}");
        let rules = &c.link.rules;
        let lc = LineCol {
            line: "7".into(),
            col: String::new(),
        };
        let printed = on_found(
            &Asked::Printed(lc),
            b"/etc/hosts",
            &crate::link::MachineNames::NONE,
            rules,
        );
        let Found::Window(Ok(run)) = printed else {
            panic!("a window: {printed:?}");
        };
        assert_eq!(run.argv(), [echo.as_str(), "/etc/hosts:7"]);
        let link = on_found(
            &Asked::Link,
            b"/etc/hosts",
            &crate::link::MachineNames::NONE,
            rules,
        );
        assert!(matches!(link, Found::Reveal), "a link reveals: {link:?}");
    }

    /// **A strike comes only from `ENOENT` or `ENOTDIR` on output known to be
    /// this machine's**: an answer baud cannot stand behind (a refused mount,
    /// an error other than those two -- `Probe::Unknown` either way), no
    /// answer yet, `ssh` in front, and a window whose worker hung each leave
    /// the link underlined and nothing struck. Perturbations: strike on
    /// `Probe::Unknown`; strike an unanswered link; drop the `readable_on`
    /// test from `file_link`; keep the marked answers in `drop_all`.
    #[test]
    fn only_a_file_found_gone_here_is_struck() {
        let text = marked("file://here.example/x", "x");
        let (lines, strikes, _, _) = struck_scan(text.as_bytes(), &|_| Probe::Unknown, &|_| {});
        assert_eq!(
            (lines, strikes),
            (vec![(0, 0, 0)], vec![]),
            "unknown: no mark"
        );
        // No answer yet: a frame owes a batch and strikes nothing.
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, text.as_bytes());
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
        let mut u = Uris::default();
        u.set_names(crate::link::MachineNames::one(HERE));
        let mut l = Lines::default();
        let mut scan = |p: &mut Paths, l: &mut Lines| {
            l.forget();
            let _ = l.scan(
                &t,
                at(),
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: p,
                },
            );
        };
        scan(&mut p, &mut l);
        assert_eq!(p.owed(), Owed::Yes, "an unanswered link owes a batch");
        assert!(l.struck().is_empty(), "and is not struck");
        // `ssh` in front: nothing asked, nothing struck.
        p.set_front(
            Front::Elsewhere,
            on_primary(at().line(Rows::ZERO, baud_domain::Row::ZERO)),
        );
        p.begin_collect();
        scan(&mut p, &mut l);
        assert!(p.end_collect().is_empty(), "nothing asked under ssh");
        assert!(l.struck().is_empty());
        // The control, and then a hang: the strike goes with the marks.
        let (_, strikes, _, mut p) = struck_scan(text.as_bytes(), &|_| Probe::Missing, &|_| {});
        assert_eq!(strikes, [(0, 0, 0)], "the control: gone is struck");
        p.close();
        assert_eq!(p.result(b"/x"), None, "a hang drops the answer");
        let mut l = Lines::default();
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::On,
                paths: &mut p,
            },
        );
        assert!(l.struck().is_empty(), "closed: nothing struck");
    }

    /// **`ls --hyperlink` naming another machine is never struck, nor looked
    /// up.** The bytes are what coreutils 9.10's `ls --hyperlink=always -1`
    /// writes into a pipe for host `remote.example` and folder `/home/user`
    /// (`[[source: coreutils-v9-10-ls-c]]`, `src/ls.c`): `quote_name` prints
    /// the opening link with `printf` at line 4726, `file://`, the host and
    /// the absolute path, then `ESC \`; each goes through `file_escape`
    /// (4669-4688), which writes every byte outside `RFC3986`'s unreserved
    /// set (1306-1312) as a lowercase `%xx`, so the space is `%20`; then the
    /// name, unquoted off a terminal (2338-2346); then `ESC ] 8 ; ; ESC \`
    /// (4743). Those lines, compiled and run on these hosts and paths, write
    /// these bytes exactly. A run here would carry this machine's name. This
    /// machine is `here.example`, so the host is another's:
    /// `admit` refuses it, nothing is asked, and nothing is drawn, whatever
    /// a worker would say. Perturbations: let `file_path` take any host; strike
    /// a link `admit` refuses.
    #[test]
    fn ls_hyperlink_from_another_host_is_never_struck() {
        let listing =
            b"\x1b]8;;file://remote.example/home/user/a%20b.md\x1b\\a b.md\x1b]8;;\x1b\\\n\
            \x1b]8;;file://remote.example/home/user/notes.txt\x1b\\notes.txt\x1b]8;;\x1b\\\n";
        let (lines, strikes, asked, p) = struck_scan(listing, &|_| Probe::Missing, &|_| {});
        assert!(asked.is_empty(), "another host's file is not looked up");
        assert!(strikes.is_empty(), "and not struck");
        assert!(
            lines.is_empty(),
            "and drawn as no link, as admit refuses it"
        );
        assert_eq!(p.result(b"/home/user/notes.txt"), None);
        assert_eq!(
            file_path(
                b"file://remote.example/home/user/notes.txt",
                &crate::link::MachineNames::one(HERE)
            ),
            None
        );
        // The control: the same listing from this machine is asked about,
        // its space decoded.
        let here = b"\x1b]8;;file://here.example/home/user/a%20b.md\x1b\\a b.md\x1b]8;;\x1b\\\n";
        let (_, strikes, asked, _) = struck_scan(here, &|_| Probe::Missing, &|_| {});
        assert_eq!(asked, [b"/home/user/a b.md".to_vec()]);
        assert_eq!(strikes, [(0, 0, 5)]);
    }

    /// **`file:///x` and `file://localhost/x` name no machine, so a gone one
    /// is never struck at rest**: it keeps its line; under the chord it is
    /// struck and the bar says it is not on this machine; without the chord
    /// it is not. Perturbations: strike `Named::NoMachine` at rest; drop the
    /// gone arm in `Hover::update_with`.
    #[test]
    fn a_file_naming_no_machine_is_struck_only_under_the_chord() {
        use crate::link::{Chord, Hover, Spot, hovered, preview_text_with};
        use baud_core::Focus;
        use baud_domain::{Col, Row};
        for uri in ["file:///gone", "file://localhost/gone"] {
            let text = marked(uri, "gone");
            let (lines, strikes, asked, p) =
                struck_scan(text.as_bytes(), &|_| Probe::Missing, &|_| {});
            assert_eq!(asked, [b"/gone".to_vec()], "{uri}: asked about");
            assert!(strikes.is_empty(), "{uri}: never struck at rest");
            assert_eq!(lines, [(0, 0, 3)], "{uri}: its line stays");
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            feed(&mut t, text.as_bytes());
            let mut h = Hover::default();
            let at_cell = Some((Row::ZERO, Col::at_index(1)));
            let _ = h.update_with(
                &t,
                at_cell,
                at(),
                Chord::Other,
                Focus::Gained,
                &crate::link::MachineNames::one(HERE),
                &p,
            );
            assert!(h.struck().is_empty(), "{uri}: no chord, no strike");
            let _ = h.update_with(
                &t,
                at_cell,
                at(),
                Chord::Cmd,
                Focus::Gained,
                &crate::link::MachineNames::one(HERE),
                &p,
            );
            assert_eq!(h.struck().len(), 1, "{uri}: struck under the chord");
            assert_eq!(h.take_entered(), Some(Gone::File(b"/gone".to_vec())));
            let spot = Spot {
                scrolled: at(),
                back: Rows::ZERO,
                row: Row::ZERO,
                col: Col::at_index(1),
            };
            let link = hovered(&t, spot).expect("the link");
            let printed = p.printed(&t, &link);
            let bar = preview_text_with(
                &link,
                &t,
                &crate::link::MachineNames::one(HERE),
                Cols::of_index(58),
                &p,
                printed,
            )
            .expect("a bar");
            assert!(bar.contains(&format!("no such file on {MACHINE}")), "{bar}");
            assert!(
                h.look().is_some_and(|l| l.spans.is_empty()),
                "nothing drawn as openable"
            );
        }
    }

    /// **A struck link selects and copies as it is written**: the strike is
    /// the frame's, so the cells under it and the text a selection over them
    /// copies are what the program wrote, and `DECRQCRA` answers as it did
    /// before the strike. Perturbations: write the strike into the cells;
    /// blank them under the strike.
    #[test]
    fn a_struck_link_copies_as_written_and_reads_back_the_same() {
        use baud_render::{Grain, Point as Pt, Selection, Span};
        let checksum = |t: &mut Term<'_>| {
            t.clear_reply();
            feed(t, b"\x1b[7;1;1;1;6;60*y");
            t.reply().to_vec()
        };
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let text = format!("\x1b[?25l{}", marked("file://here.example/gone", "gone"));
        feed(&mut t, text.as_bytes());
        let before = checksum(&mut t);
        assert!(!before.is_empty(), "the control: the query answers");
        let cells = t.view(Rows::ZERO).row(baud_domain::Row::ZERO).to_vec();
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
        let mut u = Uris::default();
        u.set_names(crate::link::MachineNames::one(HERE));
        let mut l = Lines::default();
        p.begin_collect();
        let mut scan = |p: &mut Paths, l: &mut Lines| {
            let _ = l.scan(
                &t,
                at(),
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: p,
                },
            );
        };
        scan(&mut p, &mut l);
        let asked = p.end_collect();
        let _ = p.merge(asked.into_iter().map(|c| (c.path, Probe::Missing)));
        l.forget();
        scan(&mut p, &mut l);
        assert_eq!(struck(&l), [(0, 0, 3)], "the control: it is struck");
        assert_eq!(
            t.view(Rows::ZERO).row(baud_domain::Row::ZERO),
            &cells[..],
            "no cell changed"
        );
        let line = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        let cell = |c: usize| {
            let p = Pt::new(line, baud_domain::Col::at_index(c));
            Span { start: p, end: p }
        };
        let mut sel = Selection::at(cell(0), Grain::Cell);
        sel.extend(cell(3));
        assert_eq!(sel.text(&t, at()), "gone", "copied as written");
        assert_eq!(
            checksum(&mut t),
            before,
            "DECRQCRA: the same with it struck"
        );
    }

    /// **A link whose kind has no app is struck; one baud cannot tell about,
    /// or that has one, is underlined**, and with `missing = "off"` nothing is
    /// struck; a scheme `admit` refuses is no link at all, struck or not, as
    /// before. The bar says which app is missing. Perturbations: strike on
    /// `Handler::Unknown`; read `missing` nowhere in `program_mark`.
    #[test]
    fn a_kind_with_no_app_is_struck_and_an_unknown_one_is_not() {
        let text = b"https://a.example/ http://b.example/ tel:+15550100";
        let none = |p: &mut Paths| {
            let _ = p.set_programs(vec![
                (Program::Scheme(Scheme::Https), Handler::Missing),
                (Program::Scheme(Scheme::Http), Handler::Found),
            ]);
        };
        let (lines, strikes, _, p) = struck_scan(text, &|_| Probe::Missing, &none);
        assert_eq!(strikes, [(0, 0, 17)], "https: no app, struck");
        assert_eq!(
            lines,
            [(0, 19, 35)],
            "http: an app, underlined; tel: refused, nothing"
        );
        let t = admit(b"https://a.example/", &crate::link::MachineNames::NONE).unwrap();
        assert_eq!(PathsAt::gone(&p, &t), Some(Gone::NoApp(Scheme::Https)));
        assert_eq!(
            crate::link::refusal_words(crate::link::Refused::NoApp(Scheme::Https)),
            "no app for https:"
        );
        let (lines, strikes, _, _) = struck_scan(text, &|_| Probe::Missing, &|_| {});
        assert!(strikes.is_empty(), "unknown: nothing struck");
        assert_eq!(lines.len(), 2, "both underlined");
        let (_, strikes, _, _) = struck_scan(text, &|_| Probe::Missing, &|p| {
            none(p);
            let _ = p.set_missing(Turned::Off);
        });
        assert!(strikes.is_empty(), "missing off: nothing struck");
        // With no worker to ask again, what it found goes with it.
        let (_, _, _, mut p) = struck_scan(text, &|_| Probe::Missing, &none);
        assert_eq!(p.scheme_mark(Scheme::Https), Mark::Struck, "the control");
        p.set(Turned::Off);
        assert_eq!(p.scheme_mark(Scheme::Https), Mark::Line, "paths off");
        let (_, _, _, mut p) = struck_scan(text, &|_| Probe::Missing, &none);
        p.close();
        assert_eq!(p.scheme_mark(Scheme::Https), Mark::Line, "a hang");
    }

    /// **`missing = "off"` strikes nothing under the chord either**: a printed
    /// path found gone keeps the bar's words and gets no strike, and with it
    /// on it is struck. Perturbation: drop the `strikes` test in
    /// `Hover::update_with`.
    #[test]
    fn missing_off_strikes_nothing_under_the_chord() {
        use crate::link::{Chord, Hover};
        use baud_core::Focus;
        use baud_domain::{Col, Row};
        let (_, mut p) = scanned(b"x /nope", &[]);
        assert_eq!(p.result(b"/nope"), Some(Probe::Missing), "the control");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"x /nope");
        let over = Some((Row::ZERO, Col::at_index(4)));
        let mut h = Hover::default();
        let _ = h.update_with(
            &t,
            over,
            at(),
            Chord::Cmd,
            Focus::Gained,
            &crate::link::MachineNames::NONE,
            &p,
        );
        assert_eq!(h.struck().len(), 1, "on: struck under the chord");
        let _ = p.set_missing(Turned::Off);
        let _ = h.update_with(
            &t,
            over,
            at(),
            Chord::Cmd,
            Focus::Gained,
            &crate::link::MachineNames::NONE,
            &p,
        );
        assert!(h.struck().is_empty(), "off: not struck");
        assert!(h.look().is_some(), "and the bar still says why");
    }

    /// **The hook a `command` rule reads**: its `command[0]` found missing is
    /// struck, found or unknown is not, and nothing is with `missing = "off"`.
    /// Perturbation: compare programs by kind only in `program`.
    #[test]
    fn a_commands_program_missing_marks_it_struck() {
        let mut p = Paths::new(None, Forms::DEFAULT);
        let gone = Program::Command("/no/such/editor".to_owned());
        let there = Program::Command("/usr/bin/true".to_owned());
        assert_eq!(p.program_mark(&gone), Mark::Line, "not asked yet");
        assert!(!p.programs_missing());
        assert_eq!(
            p.set_programs(vec![
                (gone.clone(), Handler::Missing),
                (there.clone(), Handler::Found)
            ]),
            Moved::Yes
        );
        assert_eq!(p.program_mark(&gone), Mark::Struck);
        assert!(p.programs_missing(), "what a job's end asks about again");
        assert_eq!(p.program_mark(&there), Mark::Line);
        assert_eq!(
            p.program_mark(&Program::Command("/other".to_owned())),
            Mark::Line,
            "one never asked about"
        );
        assert_eq!(
            p.set_programs(vec![(gone.clone(), Handler::Missing)]),
            Moved::No
        );
        let _ = p.set_missing(Turned::Off);
        assert_eq!(p.program_mark(&gone), Mark::Line, "missing off");
    }

    /// **A `[link.NAME]` rule whose `command[0]` is missing is struck at
    /// rest, and one whose program is there is not**: the config's rules feed
    /// the worker's list (`config::Link::programs`), each program is looked
    /// up as the worker does (`probe::command`, on this machine's file
    /// system), and the scan marks a rule's links by its program
    /// (`Paths::target_mark`). The missing program is a name in a fresh,
    /// empty folder; the one there is `/bin/sh`. Named break: in
    /// `target_mark`, mark every target by its scheme
    /// (`self.scheme_mark(target.scheme())`), and `tel:` is underlined, not
    /// struck.
    #[test]
    fn a_rules_missing_command_is_struck_at_rest_and_a_found_one_is_not() {
        use crate::probe::{Fs, System};
        let dir = std::env::temp_dir().join(format!("baud-strike-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a fresh folder");
        let gone = dir.join("no-such-program");
        let gone = gone.to_str().expect("a UTF-8 temp path");
        let mut c = crate::config::Config::defaults();
        let _ = c.apply(&format!(
            "[link.tel]\ncommand = [\"{gone}\", \"{{url}}\"]\n\n\
             [link.sip]\ncommand = [\"/bin/sh\", \"-c\", \"true\", \"{{url}}\"]\n"
        ));
        let asked = c.link.programs();
        let (missing, there) = (
            Program::Command(gone.to_owned()),
            Program::Command("/bin/sh".to_owned()),
        );
        assert!(asked.contains(&missing), "tel's command asked: {asked:?}");
        assert!(asked.contains(&there), "sip's command asked: {asked:?}");
        let mut fs = System;
        let mounts = fs.mounts().expect("the mount table");
        let found = |prog: &str, fs: &mut System| {
            crate::probe::command(fs, &mounts, (prog, None), &mut 0, &|| {})
        };
        assert_eq!(found(gone, &mut fs), Handler::Missing, "the control: gone");
        assert_eq!(
            found("/bin/sh", &mut fs),
            Handler::Found,
            "the control: there"
        );
        let answers = asked
            .iter()
            .filter_map(|p| match p {
                Program::Command(prog) => Some((p.clone(), found(prog, &mut fs))),
                Program::Scheme(_) => None,
            })
            .collect();
        let _ = std::fs::remove_dir(&dir);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"tel:+15550100 sip:alice@b.example");
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.set_rules(c.link.rules.clone());
        let _ = p.set_programs(answers);
        let mut u = Uris::default();
        u.set_rules(c.link.rules.clone());
        let mut l = Lines::default();
        let _ = l.scan(
            &t,
            at(),
            &mut Links {
                uris: &mut u,
                urls: Turned::On,
                paths: &mut p,
            },
        );
        assert_eq!(struck(&l), [(0, 0, 12)], "tel: its program missing, struck");
        assert_eq!(
            runs(&l),
            [(0, 14, 32)],
            "sip: its program there, underlined"
        );
    }

    /// **A `copy` rule's link is never struck**: a click copies, so an
    /// `https` link a `[link.https] action = "copy"` rule takes is drawn
    /// with a line where no browser was found, and is not gone under the
    /// chord; the same link with no rule is struck. Named break: in
    /// `target_mark`, drop the `Act::Copy` arm, and the copy rule's link is
    /// struck.
    #[test]
    fn a_copy_rule_is_never_struck_for_a_missing_app() {
        let mut c = crate::config::Config::defaults();
        let said = c.apply("[link.https]\naction = \"copy\"\n");
        assert!(said.is_empty(), "{said:?}");
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.set_rules(c.link.rules.clone());
        let _ = p.set_programs(vec![(Program::Scheme(Scheme::Https), Handler::Missing)]);
        let uri = b"https://a.example/";
        let plain = admit(uri, &crate::link::MachineNames::NONE).expect("admitted");
        assert_eq!(
            p.target_mark(&plain),
            Mark::Struck,
            "the control: no browser, struck"
        );
        let copied = crate::link::admit_link(
            uri,
            &crate::link::MachineNames::NONE,
            &c.link.rules,
            LinkOrigin::given(Origin::Local),
        )
        .expect("the rule's");
        assert_eq!(copied.action(), crate::link::Action::Copy);
        assert_eq!(p.target_mark(&copied), Mark::Line, "a copy: a line");
        assert_eq!(
            PathsAt::gone(&p, &copied),
            None,
            "and not gone under the chord"
        );
    }

    /// **Under the chord, a command or window rule's link whose program is
    /// missing says why**: `gone` names the missing command, which the bar
    /// reads as "no app for its command"; found, it is not gone. A window
    /// rule's program is drawn and said as a command's, since a click runs
    /// it the same. Named breaks: in `gone`, take a command rule's strike as
    /// `Gone::NoApp`, which names no command; in `target_mark`, draw a
    /// window rule's link by its scheme -- the window case is never struck.
    #[test]
    fn a_missing_command_says_why_under_the_chord() {
        for action in ["", "action = \"window\"\n"] {
            let mut c = crate::config::Config::defaults();
            let said = c.apply(&format!(
                "[link.tel]\n{action}command = [\"/no/such/dialer\", \"{{url}}\"]\n"
            ));
            assert!(
                !said.is_empty(),
                "the reader says the program is not there: {said:?}"
            );
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            p.set_rules(c.link.rules.clone());
            let gone = Program::Command("/no/such/dialer".to_owned());
            let t = crate::link::admit_link(
                b"tel:+15550100",
                &crate::link::MachineNames::NONE,
                &c.link.rules,
                LinkOrigin::given(Origin::Local),
            )
            .expect("the rule's");
            assert_eq!(PathsAt::gone(&p, &t), None, "the control: not asked yet");
            let _ = p.set_programs(vec![(gone.clone(), Handler::Missing)]);
            let g = PathsAt::gone(&p, &t).expect("gone");
            assert_eq!(g, Gone::NoCommand);
            let words = crate::link::refusal_words(match g {
                Gone::NoCommand => crate::link::Refused::NoCommand,
                other => panic!("{other:?}"),
            });
            assert_eq!(words, "no app for its command");
            let _ = p.set_programs(vec![(gone, Handler::Found)]);
            assert_eq!(PathsAt::gone(&p, &t), None, "found: not gone");
        }
    }

    /// **A strike survives every batch while its link is on the screen, and
    /// one asked again stays drawn until the answer comes**: two collections
    /// of the same screen keep it, a batch that answers the same moves no
    /// generation, and `recheck` owes a batch that asks it again, after which
    /// an answer of found takes the strike off. Perturbations: leave the marked
    /// answers out of `end_collect`; drop the `Stale` answer in `recheck`.
    #[test]
    fn a_strike_survives_batches_and_a_recheck_keeps_it_till_answered() {
        let text = marked("file://here.example/gone", "gone");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, text.as_bytes());
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.all_written_here();
        p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
        let mut u = Uris::default();
        u.set_names(crate::link::MachineNames::one(HERE));
        let mut l = Lines::default();
        let scan = |p: &mut Paths, l: &mut Lines, u: &mut Uris| {
            l.forget();
            let _ = l.scan(
                &t,
                at(),
                &mut Links {
                    uris: u,
                    urls: Turned::On,
                    paths: p,
                },
            );
        };
        let batch = |p: &mut Paths, l: &mut Lines, u: &mut Uris, answer: Probe| {
            p.begin_collect();
            scan(p, l, u);
            let asked = p.end_collect();
            let n = asked.len();
            let moved = p.merge(asked.into_iter().map(|c| (c.path, answer)));
            scan(p, l, u);
            (n, moved)
        };
        assert_eq!(
            batch(&mut p, &mut l, &mut u, Probe::Missing),
            (1, Moved::Yes)
        );
        assert_eq!(struck(&l), [(0, 0, 3)]);
        let g = p.generation();
        assert_eq!(
            batch(&mut p, &mut l, &mut u, Probe::Missing),
            (0, Moved::No),
            "kept, not asked"
        );
        assert_eq!(struck(&l), [(0, 0, 3)], "still struck after a second batch");
        assert_eq!(p.generation(), g);
        p.recheck();
        assert_eq!(p.owed(), Owed::Yes, "a recheck owes a batch");
        scan(&mut p, &mut l, &mut u);
        assert_eq!(struck(&l), [(0, 0, 3)], "drawn as it was until answered");
        assert_eq!(
            batch(&mut p, &mut l, &mut u, Probe::Missing),
            (1, Moved::No),
            "asked again, the same"
        );
        p.recheck_path(b"/gone");
        assert_eq!(
            batch(&mut p, &mut l, &mut u, Probe::Exists),
            (1, Moved::Yes)
        );
        assert!(struck(&l).is_empty(), "back: not struck");
        assert_eq!(runs(&l), [(0, 0, 3)], "and underlined");
    }

    /// **A `file://` name is looked up only as a plain path**: decoded, and
    /// refused with a query or fragment, a bad `%`, bytes not UTF-8, or a
    /// control or bidirectional control in it. Perturbation: keep `%00`.
    #[test]
    fn a_file_name_is_decoded_and_refused_with_a_control() {
        let here = crate::link::MachineNames::one(HERE);
        let ok = |u: &str| file_path(u.as_bytes(), &here);
        assert_eq!(
            ok("file:///a%20b"),
            Some((Named::NoMachine, b"/a b".to_vec()))
        );
        assert_eq!(
            ok("FILE://Here.Example/x"),
            Some((Named::ThisMachine, b"/x".to_vec()))
        );
        assert_eq!(
            ok("file://localhost/x"),
            Some((Named::NoMachine, b"/x".to_vec()))
        );
        for bad in [
            "file:///a?b",
            "file:///a#b",
            "file:///a%2",
            "file:///a%zz",
            "file:///a%00b",
            "file:///a%0Ab",
            "file:///a%C2%85b",
            "file:///a%E2%80%AEb",
            "file:///a%FFb",
            "file://other.example/x",
            "https://here.example/x",
        ] {
            assert_eq!(ok(bad), None, "{bad}");
        }
    }

    /// **The frame's answer is `admit`'s**: for paths short, long, encoded and
    /// not absolute, `opens_as_uri` says what `admit` says of `file_uri`.
    /// Perturbation: count an encoded byte as one in `opens_as_uri`.
    #[test]
    fn opening_is_what_admit_answers() {
        let long = format!("/{}", "a".repeat(MAX - 8));
        let longer = format!("/{}", "a".repeat(MAX - 7));
        let encoded = format!("/{}", "\u{e9}".repeat(41));
        let over = format!("/{}", "\u{e9}".repeat(42));
        for p in [
            "/etc/hosts",
            "/a b",
            "relative",
            "",
            long.as_str(),
            longer.as_str(),
            encoded.as_str(),
            over.as_str(),
        ] {
            let want = admit(
                file_uri(p.as_bytes()).as_bytes(),
                &crate::link::MachineNames::NONE,
            )
            .is_ok();
            assert_eq!(opens_as_uri(p.as_bytes()), want, "{p:?}");
        }
        assert!(opens_as_uri(long.as_bytes()), "the control: at the cap");
        assert!(!opens_as_uri(longer.as_bytes()), "the control: past it");
    }

    /// The click both drivers make on an `OSC 8` link at `line`:
    /// `link::admit_link` with the line's origin, then `link::click`.
    fn clicked_at(
        uri: &[u8],
        p: &Paths,
        line: AbsLine,
    ) -> Result<crate::link::Click, crate::link::Refused> {
        crate::link::admit_link(
            uri,
            &crate::link::MachineNames::one(HERE),
            p.rules(),
            LinkOrigin::given(p.origin(line)),
        )
        .map(|t| crate::link::click(&t, p))
    }

    /// The URI of the `OSC 8` link at `row`'s second cell.
    fn link_at(t: &Term<'_>, row: baud_domain::Row) -> Vec<u8> {
        use crate::link::{Spot, Under, hovered};
        let spot = Spot {
            scrolled: at(),
            back: Rows::ZERO,
            row,
            col: baud_domain::Col::at_index(1),
        };
        let Some(Under::Osc8(link)) = hovered(t, spot) else {
            panic!("{row:?}: the cell's link");
        };
        link.uri(t).expect("its URI").to_vec()
    }

    /// **F-local, closed: a `file://` link printed while `ssh` was in front
    /// names the other machine's file, and is refused here**, whatever host
    /// it names among this machine's -- the empty one, `localhost`, its own
    /// name -- while `ssh` is in front and from scrollback after it ends. The
    /// bytes go through the core and the URI is read back from the cell. The
    /// grid is hosts {empty, localhost, this machine, another} x the front
    /// that printed it {Local, Elsewhere, Multiplexer}; under a local front
    /// and a multiplexer this machine's links are revealed, and another
    /// machine's host is refused under every front, as before. Named break:
    /// decide from the front at the click -- the link stays refused only
    /// while `ssh` is in front, and opens from scrollback.
    #[test]
    fn a_file_link_printed_under_ssh_is_refused_here() {
        use crate::link::{Click, Refused};
        let hosts = ["", "localhost", HERE, "other.example"];
        let fronts = [Front::Local, Front::Elsewhere, Front::Multiplexer];
        for h in hosts {
            for front in fronts {
                let mut owned = Owned::new(&WS);
                let mut t = Term::new(owned.storage(), &WS);
                let mut p = Paths::new(None, Forms::DEFAULT);
                p.set(Turned::On);
                p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
                let line = at().line(Rows::ZERO, baud_domain::Row::ZERO);
                let uri = format!("file://{h}/etc/hosts");
                feed(&mut t, marked(&uri, "hosts").as_bytes());
                // Named as the worker names the read's group.
                p.written_under(Seen::Named(front), LineSpan::of(line, line));
                p.set_front(front, on_primary(line));
                let read = link_at(&t, baud_domain::Row::ZERO);
                assert_eq!(read, uri.as_bytes(), "the bytes the core kept");
                let in_front = clicked_at(&read, &p, line);
                // It ends: the shell is in front again, and the link is
                // clicked from what it printed.
                p.written_under(
                    Seen::Named(Front::Local),
                    LineSpan::of(line.below(), line.below()),
                );
                p.set_front(Front::Local, on_primary(line.below()));
                let after = clicked_at(&read, &p, line);
                let want = match (h, front) {
                    ("other.example", _) => Err(Refused::Host),
                    (_, Front::Elsewhere) => Err(Refused::Remote),
                    _ => Ok(Click::Reveal),
                };
                assert_eq!(
                    (in_front, after),
                    (want.clone(), want),
                    "{h:?} under {front:?}"
                );
            }
        }
    }

    /// **DT-L1: what a link read off the screen looks like agrees with what
    /// a click on it does, for every URI form and every origin**: over hosts
    /// {empty, `localhost`, this machine} x the origin of the program that
    /// printed it {local, remote, not named} x its form {marked with `OSC 8`,
    /// written as plain text}, its mark at rest (`Links::marked`, or the
    /// scan's line for the plain one), the preview's verdict
    /// (`link::preview_text_with` with `Paths::printed`, which hover's hand
    /// pointer reads) and the click (`link::click_on`, both drivers' one
    /// decision) agree: a link a click refuses draws no line and the
    /// preview says it won't open. The click opens only on a local line.
    /// Named breaks: the mark at rest judged without the line's origin
    /// (`Links::marked`) -- the remote `OSC 8` link draws a line; the plain
    /// URI's mark without it (`Links::read`) -- the remote plain one does.
    #[test]
    fn a_links_look_agrees_with_its_click_for_every_form_and_origin() {
        use crate::lined::Mark;
        let hosts = ["", "localhost", HERE];
        let origins = [
            (Origin::Local, Some(Seen::Named(Front::Local))),
            (Origin::Remote, Some(Seen::Named(Front::Elsewhere))),
            (Origin::Unknown, Some(Seen::NotNamed)),
        ];
        for h in hosts {
            for (origin, seen) in origins {
                for osc8 in [true, false] {
                    let mut owned = Owned::new(&WS);
                    let mut t = Term::new(owned.storage(), &WS);
                    let mut p = Paths::new(None, Forms::DEFAULT);
                    p.set(Turned::On);
                    p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
                    let mut uris = crate::lined::Uris::default();
                    uris.set_names(crate::link::MachineNames::one(HERE));
                    let line = at().line(Rows::ZERO, baud_domain::Row::ZERO);
                    let uri = format!("file://{h}/etc/hosts");
                    let text = match osc8 {
                        true => format!(" {}", marked(&uri, "hosts")),
                        false => format!(" {uri}"),
                    };
                    feed(&mut t, text.as_bytes());
                    if let Some(seen) = seen {
                        p.written_under(seen, LineSpan::of(line, line));
                    }
                    let spot = crate::link::Spot {
                        scrolled: at(),
                        back: Rows::ZERO,
                        row: baud_domain::Row::ZERO,
                        col: baud_domain::Col::at_index(1),
                    };
                    let under = crate::link::hovered(&t, spot).expect("the link");
                    let click = crate::link::clicked(
                        &under,
                        &t,
                        &crate::link::MachineNames::one(HERE),
                        p.rules(),
                        &p,
                    );
                    // The look at rest, as the scan draws it, for either form.
                    let mark = {
                        let mut l = Lines::default();
                        let _ = l.scan(
                            &t,
                            at(),
                            &mut Links {
                                uris: &mut uris,
                                urls: Turned::On,
                                paths: &mut p,
                            },
                        );
                        (!l.lines().is_empty()).then_some(Mark::Line)
                    };
                    let preview = crate::link::preview_text_with(
                        &under,
                        &t,
                        &crate::link::MachineNames::one(HERE),
                        baud_domain::Cols::of_index(120),
                        &p,
                        p.printed(&t, &under),
                    )
                    .expect("a preview");
                    let opens = click.is_ok();
                    let what = format!(
                        "{h:?}, {origin:?}, OSC 8 {osc8}: click {click:?}, mark {mark:?}, \
                         preview {preview:?}"
                    );
                    assert_eq!(
                        (mark == Some(Mark::Line), !preview.starts_with("Won't open")),
                        (opens, opens),
                        "{what}"
                    );
                    assert_eq!(opens, origin == Origin::Local, "{what}");
                }
            }
        }
    }

    /// **B1: a link over two rows looks as its click acts on each row**: an
    /// `OSC 8` link and a plain `file://` URI, each wrapped from a row a
    /// local program wrote onto a row nothing was filed over, or one a
    /// writer gone before it was named wrote, draw no line on either row,
    /// say they won't open in the preview, and are refused by a click on
    /// either row, since a link is judged over every row it covers. The
    /// control: both rows local, lit and opened on both. Named breaks:
    /// judge an `OSC 8` link over the row read alone -- its first row
    /// lights while its click refuses; judge a bare URI's click by the row
    /// clicked (`Bare::lines` of one row) -- the first row's click opens
    /// while nothing lights.
    #[test]
    fn a_link_over_two_rows_looks_as_its_click_acts_on_each() {
        let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        let second = first.below();
        for osc8 in [true, false] {
            for (case, below) in [
                ("not filed", None),
                ("not named", Some(Seen::NotNamed)),
                ("the control", Some(Seen::Named(Front::Local))),
            ] {
                let mut owned = Owned::new(&WS);
                let mut t = Term::new(owned.storage(), &WS);
                let mut p = Paths::new(None, Forms::DEFAULT);
                p.set(Turned::On);
                p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
                let mut uris = crate::lined::Uris::default();
                uris.set_names(crate::link::MachineNames::one(HERE));
                let text = match osc8 {
                    true => format!(
                        "{}{}",
                        " ".repeat(54),
                        marked("file:///etc/hosts", "abcdefghij")
                    ),
                    false => format!("{}file:///etc/hosts", " ".repeat(50)),
                };
                feed(&mut t, text.as_bytes());
                p.written_under(Seen::Named(Front::Local), LineSpan::of(first, first));
                if let Some(seen) = below {
                    p.written_under(seen, LineSpan::of(second, second));
                }
                let mut l = Lines::default();
                let _ = l.scan(
                    &t,
                    at(),
                    &mut Links {
                        uris: &mut uris,
                        urls: Turned::On,
                        paths: &mut p,
                    },
                );
                let lit = runs(&l);
                for (row, col) in [(0, 56), (1, 1)] {
                    let spot = crate::link::Spot {
                        scrolled: at(),
                        back: Rows::ZERO,
                        row: baud_domain::Row::from_index(row),
                        col: baud_domain::Col::at_index(col),
                    };
                    let under = crate::link::hovered(&t, spot).expect("the link");
                    let click = crate::link::clicked(
                        &under,
                        &t,
                        &crate::link::MachineNames::one(HERE),
                        p.rules(),
                        &p,
                    );
                    let preview = crate::link::preview_text_with(
                        &under,
                        &t,
                        &crate::link::MachineNames::one(HERE),
                        baud_domain::Cols::of_index(120),
                        &p,
                        p.printed(&t, &under),
                    )
                    .expect("a preview");
                    let shows = lit.iter().any(|r| r.0 == row);
                    let what = format!(
                        "OSC 8 {osc8}, {case}, row {row}: lit {lit:?}, click {click:?}, \
                         preview {preview:?}"
                    );
                    assert_eq!(
                        (shows, !preview.starts_with("Won't open")),
                        (click.is_ok(), click.is_ok()),
                        "{what}"
                    );
                    assert_eq!(click.is_ok(), case == "the control", "{what}");
                }
            }
        }
    }

    /// The look at rest, the preview and the click of the link at `(row,
    /// col)`, as `p` judges them: whether its row draws a line, and whether
    /// the preview and the click open it.
    fn look_preview_click(
        t: &Term<'_>,
        p: &mut Paths,
        uris: &mut crate::lined::Uris,
        (row, col): (baud_domain::Row, baud_domain::Col),
    ) -> (bool, bool, bool) {
        let mut l = Lines::default();
        let _ = l.scan(
            t,
            at(),
            &mut Links {
                uris,
                urls: Turned::On,
                paths: p,
            },
        );
        let lit = runs(&l);
        let spot = crate::link::Spot {
            scrolled: at(),
            back: Rows::ZERO,
            row,
            col,
        };
        let under = crate::link::hovered(t, spot).expect("the link");
        let click = crate::link::clicked(
            &under,
            t,
            &crate::link::MachineNames::one(HERE),
            p.rules(),
            &*p,
        );
        let preview = crate::link::preview_text_with(
            &under,
            t,
            &crate::link::MachineNames::one(HERE),
            baud_domain::Cols::of_index(120),
            &*p,
            p.printed(t, &under),
        )
        .expect("a preview");
        (
            lit.iter().any(|r| r.0 == row.index()),
            !preview.starts_with("Won't open"),
            click.is_ok(),
        )
    }

    /// **B1: one `OSC 8` slot shown on two runs gives one answer from
    /// either**: two links with no `id` and one target share a slot, the
    /// first on row 0, which a local program wrote, the second on row 2,
    /// past a plain row, which a writer not named, or a remote one, wrote.
    /// Neither row draws a line, the preview on either says it won't open,
    /// and a click on either is refused: the slot is judged over every row
    /// in view holding it, the cells its look draws. The control: row 2
    /// local too, both lit and opened. Named break: judge the slot over the
    /// row read alone -- row 0 lights and opens while row 2 does neither.
    #[test]
    fn one_slot_on_two_runs_gives_one_answer_from_either() {
        let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        let (plain, third) = (first.below(), first.below().below());
        for (case, seen) in [
            ("not named", Seen::NotNamed),
            ("remote", Seen::Named(Front::Elsewhere)),
            ("the control", Seen::Named(Front::Local)),
        ] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
            let mut uris = crate::lined::Uris::default();
            uris.set_names(crate::link::MachineNames::one(HERE));
            let uri = "file:///etc/hosts";
            let text = format!("{}\r\nplain\r\n{}", marked(uri, "aaa"), marked(uri, "bbb"));
            feed(&mut t, text.as_bytes());
            p.written_under(Seen::Named(Front::Local), LineSpan::of(first, plain));
            p.written_under(seen, LineSpan::of(third, third));
            let from_first = look_preview_click(
                &t,
                &mut p,
                &mut uris,
                (
                    baud_domain::Row::from_index(0),
                    baud_domain::Col::at_index(1),
                ),
            );
            let from_third = look_preview_click(
                &t,
                &mut p,
                &mut uris,
                (
                    baud_domain::Row::from_index(2),
                    baud_domain::Col::at_index(1),
                ),
            );
            let opens = case == "the control";
            assert_eq!(from_first, (opens, opens, opens), "{case}: row 0");
            assert_eq!(from_third, (opens, opens, opens), "{case}: row 2");
        }
    }

    /// **A slot no row in view shows, and no read filed against, is not
    /// known**: no row vouches for it. The control: one local row shows
    /// it, and it is local. Named break: fold the rows from local -- no
    /// rows read as local.
    #[test]
    fn a_slot_no_row_shows_is_not_known() {
        let mut p = Paths::new(None, Forms::DEFAULT);
        let (id, uri) = (LinkId::from_index(1), b"file:///etc/hosts".as_slice());
        assert_eq!(
            p.link_origin(Surface::Primary, id, uri, &[]).origin(),
            Origin::Unknown
        );
        let line = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        p.written_under(Seen::Named(Front::Local), LineSpan::of(line, line));
        assert_eq!(
            p.link_origin(Surface::Primary, id, uri, &[line]).origin(),
            Origin::Local
        );
    }

    /// Whether `row` draws a line once `l`, kept from scan to scan, scans
    /// the window numbered by `scrolled`, and whether a click at `(row,
    /// col)` opens the link there.
    fn look_click_kept(
        l: &mut Lines,
        t: &Term<'_>,
        p: &mut Paths,
        uris: &mut crate::lined::Uris,
        scrolled: Scrolled,
        (row, col): (baud_domain::Row, baud_domain::Col),
    ) -> (bool, bool) {
        let _ = l.scan(
            t,
            scrolled,
            &mut Links {
                uris,
                urls: Turned::On,
                paths: p,
            },
        );
        let spot = crate::link::Spot {
            scrolled,
            back: Rows::ZERO,
            row,
            col,
        };
        let under = crate::link::hovered(t, spot).expect("the link");
        let click = crate::link::clicked(
            &under,
            t,
            &crate::link::MachineNames::one(HERE),
            p.rules(),
            &*p,
        );
        (runs(l).iter().any(|r| r.0 == row.index()), click.is_ok())
    }

    /// **One slot keeps one look as the rows showing it change, from scan
    /// to scan of one window**: two links with no `id` and one target share
    /// a slot. A local program's on row 0, above two plain rows, lights;
    /// then a remote program's on row 3 joins the slot, and row 0, its
    /// cells and the row below it unchanged, goes dark as its click is
    /// refused. The other way: the local link on row 0 and
    /// the remote one on row 2 are dark; then the remote row is erased, or
    /// leaves the window as the screen scrolls, and the local row, unchanged,
    /// lights as its click opens. Named break: read again only the rows
    /// whose cells changed (no `note_slots`) -- the kept row keeps its old
    /// look.
    #[test]
    fn one_slot_keeps_one_look_as_its_rows_change() {
        let uri = "file:///etc/hosts";
        let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        let third = first.below().below();
        let fourth = third.below();
        let start = || {
            let owned = Owned::new(&WS);
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
            let mut uris = crate::lined::Uris::default();
            uris.set_names(crate::link::MachineNames::one(HERE));
            (owned, p, uris, Lines::default())
        };
        // A remote row joins the slot.
        let (mut owned, mut p, mut uris, mut l) = start();
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            format!("{}\r\nplain\r\nplain", marked(uri, "aaa")).as_bytes(),
        );
        p.written_under(Seen::Named(Front::Local), LineSpan::of(first, third));
        let lit = look_click_kept(
            &mut l,
            &t,
            &mut p,
            &mut uris,
            at(),
            (
                baud_domain::Row::from_index(0),
                baud_domain::Col::at_index(1),
            ),
        );
        assert_eq!(lit, (true, true), "the local link alone");
        feed(&mut t, format!("\r\n{}", marked(uri, "bbb")).as_bytes());
        p.written_under(Seen::Named(Front::Elsewhere), LineSpan::of(fourth, fourth));
        let joined = look_click_kept(
            &mut l,
            &t,
            &mut p,
            &mut uris,
            at(),
            (
                baud_domain::Row::from_index(0),
                baud_domain::Col::at_index(1),
            ),
        );
        assert_eq!(joined, (false, false), "a remote row joined the slot");
        // The remote row leaves, erased or scrolled out of the window.
        for leaves in ["erased", "scrolled out"] {
            let (mut owned, mut p, mut uris, mut l) = start();
            let mut t = Term::new(owned.storage(), &WS);
            let (remote, local) = match leaves {
                "erased" => (third, first),
                _ => (first, third),
            };
            let text = format!("{}\r\nplain\r\n{}", marked(uri, "aaa"), marked(uri, "bbb"));
            feed(&mut t, text.as_bytes());
            p.written_under(Seen::Named(Front::Local), LineSpan::of(first, third));
            p.written_under(Seen::Named(Front::Elsewhere), LineSpan::of(remote, remote));
            let row = |line: AbsLine| match line == first {
                true => baud_domain::Row::ZERO,
                false => baud_domain::Row::from_index(2),
            };
            let dark = look_click_kept(
                &mut l,
                &t,
                &mut p,
                &mut uris,
                at(),
                (row(local), baud_domain::Col::at_index(1)),
            );
            assert_eq!(dark, (false, false), "{leaves}: both rows shown");
            let (scrolled, row) = match leaves {
                "erased" => {
                    feed(&mut t, b"\x1b[3;1H\x1b[2Kplain");
                    (at(), baud_domain::Row::ZERO)
                }
                _ => {
                    feed(&mut t, b"\r\n\r\n\r\n\r\n");
                    (
                        at().advanced(Rows::of_count(1)),
                        baud_domain::Row::from_index(1),
                    )
                }
            };
            let after = look_click_kept(
                &mut l,
                &t,
                &mut p,
                &mut uris,
                scrolled,
                (row, baud_domain::Col::at_index(1)),
            );
            assert_eq!(after, (true, true), "{leaves}: the local row alone");
        }
    }

    /// **B1: a URI read from the text is judged over its own rows**: a
    /// plain `file://` URI on row 1 of a line wrapped from row 0, which a
    /// remote program wrote, while a local one wrote row 1, lights and
    /// opens: the remote row is the line's, not the URI's. Named break:
    /// judge a URI over its whole logical line (`Text::lines_of` returning
    /// the line's) -- it goes dark and its preview refuses.
    #[test]
    fn a_uri_on_the_local_rows_of_a_line_with_a_remote_row_opens() {
        let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
        let mut uris = crate::lined::Uris::default();
        uris.set_names(crate::link::MachineNames::one(HERE));
        let text = format!("{} file:///etc/hosts", "r".repeat(59));
        feed(&mut t, text.as_bytes());
        p.written_under(Seen::Named(Front::Elsewhere), LineSpan::of(first, first));
        p.written_under(
            Seen::Named(Front::Local),
            LineSpan::of(first.below(), first.below()),
        );
        assert_eq!(
            look_preview_click(
                &t,
                &mut p,
                &mut uris,
                (
                    baud_domain::Row::from_index(1),
                    baud_domain::Col::at_index(3)
                )
            ),
            (true, true, true),
            "lit, previewed and opened"
        );
    }

    /// **A real `ssh` session's file links are its machine's**: the bytes
    /// OpenSSH 10.0p2 wrote (`tests/ssh-link`), read while `ssh` was in
    /// front, give four links whose URIs read back from the cells; the three
    /// naming this machine are refused on a click, with `ssh` in front and
    /// after it ends, and `ls`'s own, naming the remote, is refused as any
    /// other host is. Named break: decide from the front at the click.
    #[test]
    fn a_real_ssh_sessions_file_links_are_its_machines() {
        use crate::link::Refused;
        const CAPTURE: &[u8] = include_bytes!("../../tests/ssh-link/remote-file-links.raw");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
        let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        feed(&mut t, CAPTURE);
        let last = at().line(Rows::ZERO, t.cursor().0);
        p.written_under(Seen::Named(Front::Elsewhere), LineSpan::of(first, last));
        p.set_front(Front::Elsewhere, on_primary(last));
        let want = [
            ("file:///etc/passwd", Refused::Remote),
            ("file://localhost/etc/passwd", Refused::Remote),
            ("file://here.example/etc/passwd", Refused::Remote),
            ("file://remotebox/etc/hosts", Refused::Host),
        ];
        let after = last.below();
        for ended in [false, true] {
            if ended {
                // It ends: the shell is in front again.
                p.written_under(Seen::Named(Front::Local), LineSpan::of(after, after));
                p.set_front(Front::Local, on_primary(after));
            }
            for (row, (uri, why)) in want.into_iter().enumerate() {
                let read = link_at(&t, baud_domain::Row::from_index(row));
                assert_eq!(read, uri.as_bytes(), "row {row}");
                let line = at().line(Rows::ZERO, baud_domain::Row::from_index(row));
                assert_eq!(
                    clicked_at(&read, &p, line),
                    Err(why),
                    "{uri}, ssh ended: {ended}"
                );
            }
        }
    }

    /// The click both drivers make on the link under `row`'s second cell,
    /// marked or read from the text: its URI read back from the cells, then
    /// `link::click_on`, with the origin of the rows it covers
    /// ([`PathsAt::printed`]), then `link::click`.
    fn clicked_under(
        t: &Term<'_>,
        p: &Paths,
        row: baud_domain::Row,
    ) -> (Vec<u8>, Result<crate::link::Click, crate::link::Refused>) {
        use crate::link::{Spot, Under, hovered};
        let spot = Spot {
            scrolled: at(),
            back: Rows::ZERO,
            row,
            col: baud_domain::Col::at_index(1),
        };
        let under = hovered(t, spot).expect("a link under the cell");
        let uri = match &under {
            Under::Osc8(l) => l.uri(t).expect("its URI").to_vec(),
            Under::Bare(b) => b.uri().expect("its URI").to_vec(),
            Under::Path(_) => panic!("{row:?}: a path, not a link"),
        };
        let clicked = crate::link::clicked(
            &under,
            t,
            &crate::link::MachineNames::one(HERE),
            p.rules(),
            p,
        );
        (uri, clicked)
    }

    /// **A `file://` URI a remote program printed as plain text is refused
    /// here, as a marked one is**: `file://localhost/etc/passwd` on a line
    /// `ssh` wrote, read from the text of the cells, is refused while `ssh`
    /// is in front and after it ends; on a line the shell wrote it is
    /// revealed, the control. Named break: give a link read from the text
    /// no origin in `PathsAt::printed` -- the remote line's is revealed.
    #[test]
    fn a_plain_file_uri_printed_under_ssh_is_refused_here() {
        use crate::link::{Click, Refused};
        let run = |front: Front| {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
            let line = at().line(Rows::ZERO, baud_domain::Row::ZERO);
            feed(&mut t, b"file://localhost/etc/passwd\r\n");
            p.written_under(Seen::Named(front), LineSpan::of(line, line));
            p.set_front(front, on_primary(line));
            let (uri, in_front) = clicked_under(&t, &p, baud_domain::Row::ZERO);
            assert_eq!(uri, b"file://localhost/etc/passwd", "read from the text");
            p.written_under(
                Seen::Named(Front::Local),
                LineSpan::of(line.below(), line.below()),
            );
            p.set_front(Front::Local, on_primary(line.below()));
            let (_, after) = clicked_under(&t, &p, baud_domain::Row::ZERO);
            (in_front, after)
        };
        assert_eq!(
            run(Front::Elsewhere),
            (Err(Refused::Remote), Err(Refused::Remote)),
            "printed under ssh"
        );
        assert_eq!(
            run(Front::Local),
            (Ok(Click::Reveal), Ok(Click::Reveal)),
            "the control: printed by the shell"
        );
    }

    /// **A link read from the text takes its origin as a marked one does,
    /// whatever host it names and whichever program printed it**, revealed,
    /// opened in a window by a rule, or refused: the grid is hosts {empty,
    /// localhost, this machine, another} x the front that printed it {Local,
    /// Elsewhere, Multiplexer} x the link marked with `OSC 8` or printed as
    /// plain text x `[link.file]` left alone or set to `action = "window"`,
    /// each clicked while that front is in front and again after the shell
    /// is back. A plain one answers exactly as the marked one on the same
    /// row, and neither reaches a reveal or a window from a line `ssh`
    /// wrote. Named break: give a link read from the text no origin in
    /// `PathsAt::printed` -- a plain link from `ssh` reveals, or opens its
    /// window. The rule is read under a `PATH` holding a stand-in `hx`, so
    /// the test holds whether or not helix is installed.
    #[test]
    fn a_plain_file_uri_takes_its_origin_as_a_marked_one_does() {
        use crate::link::{Click, Refused};
        use std::os::unix::fs::PermissionsExt as _;
        let bin = std::env::temp_dir().join(format!("baud-paths-hx-{}", std::process::id()));
        std::fs::create_dir_all(&bin).expect("a folder");
        let hx = bin.join("hx");
        std::fs::write(&hx, "#!/bin/sh\n").expect("a program");
        std::fs::set_permissions(&hx, std::fs::Permissions::from_mode(0o755)).expect("its mode");
        let env = crate::config::ConfigEnv {
            home: None,
            path: Some(bin.clone().into_os_string()),
        };
        let mut window = crate::config::Config::defaults();
        let said = window.apply_in(
            "[link.file]\naction = \"window\"\ncommand = [\"hx\", \"{url}\"]\n",
            &env,
        );
        let _ = std::fs::remove_dir_all(&bin);
        assert_eq!(said, Vec::<String>::new(), "the rule reads");
        let hosts = ["", "localhost", HERE, "other.example"];
        let fronts = [Front::Local, Front::Elsewhere, Front::Multiplexer];
        for rules in [None, Some(window.link.rules.clone())] {
            for h in hosts {
                for front in fronts {
                    let mut owned = Owned::new(&WS);
                    let mut t = Term::new(owned.storage(), &WS);
                    let mut p = Paths::new(None, Forms::DEFAULT);
                    p.set(Turned::On);
                    p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
                    if let Some(r) = &rules {
                        p.set_rules(r.clone());
                    }
                    let uri = format!("file://{h}/etc/hosts");
                    // Row 0 marked, row 1 the same URI as plain text.
                    feed(
                        &mut t,
                        format!("{}\r\n{uri}\r\n", marked(&uri, "hosts")).as_bytes(),
                    );
                    let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
                    let second = first.below();
                    p.written_under(Seen::Named(front), LineSpan::of(first, second));
                    p.set_front(front, on_primary(second));
                    let click = |p: &Paths| {
                        let (m, marked) = clicked_under(&t, p, baud_domain::Row::ZERO);
                        let (b, plain) = clicked_under(&t, p, baud_domain::Row::from_index(1));
                        assert_eq!(
                            (&m, &b),
                            (&uri.as_bytes().to_vec(), &uri.as_bytes().to_vec())
                        );
                        (marked, plain)
                    };
                    let in_front = click(&p);
                    let back = second.below();
                    p.written_under(Seen::Named(Front::Local), LineSpan::of(back, back));
                    p.set_front(Front::Local, on_primary(back));
                    let after = click(&p);
                    let opened = match rules {
                        None => Click::Reveal,
                        Some(_) => Click::Window,
                    };
                    let want = match (h, front) {
                        ("other.example", _) => Err(Refused::Host),
                        (_, Front::Elsewhere) => Err(Refused::Remote),
                        _ => Ok(opened),
                    };
                    let what = format!("{h:?} under {front:?}, window rule: {}", rules.is_some());
                    assert_eq!(in_front, (want.clone(), want.clone()), "{what}, in front");
                    assert_eq!(after, (want.clone(), want), "{what}, after");
                }
            }
        }
    }

    /// **A `file://` URI printed as plain text on a line no program has been
    /// named for yet is refused until it is, then revealed**, as a marked
    /// one is. Named break: give a link read from the text no origin in
    /// `PathsAt::printed` -- it is revealed before its line is named.
    #[test]
    fn a_plain_file_uri_on_an_unnamed_line_waits_for_its_name() {
        use crate::link::{Click, Refused};
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        feed(&mut t, b"file:///etc/hosts\r\n");
        let (_, unnamed) = clicked_under(&t, &p, baud_domain::Row::ZERO);
        assert_eq!(unnamed, Err(Refused::Unnamed), "no program named yet");
        let line = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        p.written_under(Seen::Named(Front::Local), LineSpan::of(line, line));
        let (_, named) = clicked_under(&t, &p, baud_domain::Row::ZERO);
        assert_eq!(named, Ok(Click::Reveal), "named the shell's");
    }

    /// **A real `ssh` session's plain `file://` URIs are its machine's, as
    /// its marked links are**: the bytes OpenSSH 10.0p2 wrote
    /// (`tests/ssh-link/remote-plain-file-uris.raw`), read while `ssh` was
    /// in front, hold four URIs as plain text, read back from the cells; the
    /// three naming this machine are refused on a click, with `ssh` in front
    /// and after it ends, and the remote's own host is refused as any other
    /// host is. Named break: give a link read from the text no origin in
    /// `PathsAt::printed` -- the three are revealed.
    #[test]
    fn a_real_ssh_sessions_plain_file_uris_are_its_machines() {
        use crate::link::Refused;
        const CAPTURE: &[u8] = include_bytes!("../../tests/ssh-link/remote-plain-file-uris.raw");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
        let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        feed(&mut t, CAPTURE);
        let last = at().line(Rows::ZERO, t.cursor().0);
        p.written_under(Seen::Named(Front::Elsewhere), LineSpan::of(first, last));
        p.set_front(Front::Elsewhere, on_primary(last));
        let want = [
            ("file:///etc/passwd", Refused::Remote),
            ("file://localhost/etc/passwd", Refused::Remote),
            ("file://here.example/etc/passwd", Refused::Remote),
            ("file://remotebox/etc/hosts", Refused::Host),
        ];
        let after = last.below();
        for ended in [false, true] {
            if ended {
                p.written_under(Seen::Named(Front::Local), LineSpan::of(after, after));
                p.set_front(Front::Local, on_primary(after));
            }
            for (row, (uri, why)) in want.into_iter().enumerate() {
                let (read, clicked) = clicked_under(&t, &p, baud_domain::Row::from_index(row));
                assert_eq!(read, uri.as_bytes(), "row {row}");
                assert_eq!(clicked, Err(why), "{uri}, ssh ended: {ended}");
            }
        }
    }

    /// **A link printed before the `ssh` is still this machine's while `ssh`
    /// is in front**, and one printed under it is not: the origin is the
    /// line's, not the front's. Named break: one watermark for every line up
    /// to the last `ssh` wrote -- the local link is refused.
    #[test]
    fn a_local_link_printed_before_ssh_opens_while_ssh_is_in_front() {
        use crate::link::{Click, Refused};
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
        let (l0, l1) = (
            at().line(Rows::ZERO, baud_domain::Row::ZERO),
            at().line(Rows::ZERO, baud_domain::Row::from_index(1)),
        );
        feed(
            &mut t,
            marked("file://here.example/etc/hosts", "hosts").as_bytes(),
        );
        feed(&mut t, b"\r\n");
        p.written_under(Seen::Named(Front::Local), LineSpan::of(l0, l0));
        feed(
            &mut t,
            marked("file://localhost/etc/hosts", "hosts").as_bytes(),
        );
        p.written_under(Seen::Named(Front::Elsewhere), LineSpan::of(l1, l1));
        p.set_front(Front::Elsewhere, on_primary(l1));
        assert_eq!(
            clicked_at(&link_at(&t, baud_domain::Row::ZERO), &p, l0),
            Ok(Click::Reveal),
            "printed here"
        );
        assert_eq!(
            clicked_at(&link_at(&t, baud_domain::Row::from_index(1)), &p, l1),
            Err(Refused::Remote),
            "printed there"
        );
    }

    /// **A link on a line no program was named for yet is refused, and
    /// opens once its program is named local**; one named remote stays
    /// refused. Named break: take an unnamed line as local.
    #[test]
    fn an_unnamed_links_origin_is_refused_until_named() {
        use crate::link::{Click, Refused};
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
        let line = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        feed(&mut t, marked("file:///etc/hosts", "hosts").as_bytes());
        let read = link_at(&t, baud_domain::Row::ZERO);
        assert_eq!(p.origin(line), Origin::Unknown);
        assert_eq!(
            clicked_at(&read, &p, line),
            Err(Refused::Unnamed),
            "not named yet"
        );
        let mut remote = Paths::new(None, Forms::DEFAULT);
        remote.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
        p.written_under(Seen::Named(Front::Local), LineSpan::of(line, line));
        assert_eq!(
            clicked_at(&read, &p, line),
            Ok(Click::Reveal),
            "named local"
        );
        remote.written_under(Seen::Named(Front::Elsewhere), LineSpan::of(line, line));
        assert_eq!(
            clicked_at(&read, &remote, line),
            Err(Refused::Remote),
            "named remote"
        );
    }

    /// **With `[link] paths = "off"` and `[link.file] action = "window"`, a
    /// remote program's `file://localhost` link opens nothing**: the rule
    /// would run your editor on this machine's file of that name. A link a
    /// local program printed takes the rule. Named break: read no origin
    /// for a link a rule takes.
    #[test]
    fn a_file_window_rule_never_opens_a_remote_programs_link() {
        use crate::link::{Click, Refused};
        let mut c = crate::config::Config::defaults();
        let said = c.apply(&format!(
            "[link]\npaths = \"off\"\n\n[link.file]\naction = \"window\"\n\
             command = [\"{}\", \"{{url}}\"]\n",
            tool("true")
        ));
        assert!(said.is_empty(), "{said:?}");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set_rules(c.link.rules.clone());
        p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
        assert_eq!(p.marks(), Marks::Off, "paths off");
        let (l0, l1) = (
            at().line(Rows::ZERO, baud_domain::Row::ZERO),
            at().line(Rows::ZERO, baud_domain::Row::from_index(1)),
        );
        feed(
            &mut t,
            marked("file://localhost/etc/hosts", "hosts").as_bytes(),
        );
        feed(&mut t, b"\r\n");
        feed(
            &mut t,
            marked("file://localhost/etc/hosts", "hosts").as_bytes(),
        );
        p.written_under(Seen::Named(Front::Elsewhere), LineSpan::of(l0, l0));
        p.written_under(Seen::Named(Front::Local), LineSpan::of(l1, l1));
        assert_eq!(
            clicked_at(&link_at(&t, baud_domain::Row::ZERO), &p, l0),
            Err(Refused::Remote),
            "remote's"
        );
        assert_eq!(
            clicked_at(&link_at(&t, baud_domain::Row::from_index(1)), &p, l1),
            Ok(Click::Window),
            "local's"
        );
    }

    /// `[link.path]` opening a window running `hx` on the path.
    const PATH_WINDOW: &str = "[link.path]\naction = \"window\"\ncommand = [\"hx\", \"{path}\"]\n";

    /// **The link rules `config` leaves over the defaults**, read as the
    /// app reads its file. A macro, expanded only inside `#[test]`
    /// functions: the `frame` bench compiles this module without them and
    /// without `config.rs`, so a helper it compiles takes the rules.
    macro_rules! rules_of {
        ($config:expr) => {{
            let mut c = crate::config::Config::defaults();
            let _ = c.apply($config);
            c.link.rules
        }};
    }

    /// **The ssh session in front**: `ssh -p 2222 -i k -L 1:h:1 demo`,
    /// which `ssh -G` resolves to `demo.example`.
    fn demo_session() -> crate::ssh::Session {
        let argv: Vec<std::ffi::OsString> = ["ssh", "-p", "2222", "-i", "k", "-L", "1:h:1", "demo"]
            .iter()
            .map(Into::into)
            .collect();
        crate::ssh::session(&argv, |_| {
            Some(b"hostname demo.example\nport 2222\n".to_vec())
        })
        .expect("a session")
    }

    /// **What the click both drivers make does with the link at `row`'s
    /// second cell** (`link::click_on`), on [`on_remote`]'s screen: `Ok`
    /// with what opening it on the session's machine comes to, the window's
    /// argv or why none opens; `Err` with the refusal of a link judged as
    /// any other is.
    fn remote_click(
        rules: crate::rules::Rules,
        bytes: &[u8],
        front: Front,
        session: Option<&crate::ssh::Session>,
        row: baud_domain::Row,
    ) -> Result<Result<Vec<String>, crate::link::Refused>, crate::link::Refused> {
        use crate::link::{ClickOn, MachineNames, click_on};
        let words = |run: crate::rules::Run| {
            run.argv()
                .iter()
                .map(|w| w.to_str().expect("UTF-8").to_owned())
                .collect()
        };
        on_remote(
            rules,
            bytes,
            front,
            session,
            row,
            |t, p, under| match click_on(under, t, &MachineNames::one(HERE), p.rules(), &*p) {
                ClickOn::Remote(built, _) => Ok(built.map(words)),
                ClickOn::Refused(why, _) => Err(why),
                other => panic!("row {row:?}: {other:?}"),
            },
        )
    }

    /// **How the link at `row`'s second cell looks and what its click
    /// does**, on [`on_remote`]'s screen: the line the scan draws on `row`
    /// at rest, if any (`Links`, over every row), the preview's bar for a
    /// wide row (`link::preview_text_with`), and the click (`link::clicked`,
    /// both drivers' decision).
    fn remote_look(
        rules: crate::rules::Rules,
        bytes: &[u8],
        session: Option<&crate::ssh::Session>,
        row: baud_domain::Row,
    ) -> (
        Option<crate::lined::Mark>,
        String,
        Result<crate::link::Click, crate::link::Refused>,
    ) {
        use crate::link::MachineNames;
        on_remote(
            rules,
            bytes,
            Front::Elsewhere,
            session,
            row,
            |t, p, under| {
                let mut uris = crate::lined::Uris::default();
                uris.set_names(MachineNames::one(HERE));
                uris.set_rules(p.rules().clone());
                let mut l = Lines::default();
                let _ = l.scan(
                    t,
                    at(),
                    &mut Links {
                        uris: &mut uris,
                        urls: Turned::On,
                        paths: &mut *p,
                    },
                );
                let lit = l
                    .lines()
                    .iter()
                    .any(|r| r.row == row)
                    .then_some(crate::lined::Mark::Line);
                let printed = p.printed(t, under);
                let preview = crate::link::preview_text_with(
                    under,
                    t,
                    &MachineNames::one(HERE),
                    baud_domain::Cols::of_index(120),
                    &*p,
                    printed,
                )
                .expect("a preview");
                let click =
                    crate::link::clicked(under, t, &MachineNames::one(HERE), p.rules(), &*p);
                (lit, preview, click)
            },
        )
    }

    /// **The screen `bytes` leave, every row written while `front` was in
    /// front**, `rules` set, this machine named [`HERE`] and `session` the
    /// ssh session the rows were printed under, still in front, as the
    /// worker read it (`Paths::written_by`, `Paths::set_session`), handed to
    /// `f` with the link at `row`'s second cell.
    fn on_remote<R>(
        rules: crate::rules::Rules,
        bytes: &[u8],
        front: Front,
        session: Option<&crate::ssh::Session>,
        row: baud_domain::Row,
        f: impl FnOnce(&Term<'_>, &mut Paths, &crate::link::Under) -> R,
    ) -> R {
        on_printed(rules, bytes, front, (session, session), row, f)
    }

    /// [`on_remote`], the rows printed under the first session and the
    /// second in front at the click.
    fn on_printed<R>(
        rules: crate::rules::Rules,
        bytes: &[u8],
        front: Front,
        (printed, in_front): (Option<&crate::ssh::Session>, Option<&crate::ssh::Session>),
        row: baud_domain::Row,
        f: impl FnOnce(&Term<'_>, &mut Paths, &crate::link::Under) -> R,
    ) -> R {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.set_rules(rules);
        p.set_names(crate::link::MachineNames::one(HERE));
        feed(&mut t, bytes);
        let last = baud_domain::Row::from_index(usize::from(WS.ws_row) - 1);
        p.written_by(
            Seen::Named(front),
            printed.cloned().map(std::sync::Arc::new).as_ref(),
            LineSpan::of(
                at().line(Rows::ZERO, baud_domain::Row::ZERO),
                at().line(Rows::ZERO, last),
            ),
        );
        p.set_session(in_front.cloned().map(std::sync::Arc::new));
        let spot = crate::link::Spot {
            scrolled: at(),
            back: Rows::ZERO,
            row,
            col: baud_domain::Col::at_index(1),
        };
        let under = crate::link::hovered(&t, spot).expect("a link under the cell");
        f(&t, &mut p, &under)
    }

    /// **A click on a remote program's `file://` link opens a window
    /// running `ssh` to the session in front**: its identity options and
    /// not its forward, `-t`, the destination as typed, and the far end's
    /// check with the link's host and `[link.path]`'s program and path, each
    /// word quoted on its own -- the `'` in the path as `'\''`, the `$` and
    /// the decoded space left inside the quotes. The control: the same link
    /// on a line the shell printed is refused here, a file on another
    /// computer. Named breaks: take the link's host as the destination --
    /// `demo.example` follows `-t`; pass the path unquoted -- the path's
    /// word has no quotes.
    #[test]
    fn a_click_on_a_remote_file_link_opens_ssh_to_the_sessions_machine() {
        let session = demo_session();
        let link = marked("file://demo.example/home/u/it's%20$x.md", "it");
        let window = remote_click(
            rules_of!(PATH_WINDOW),
            link.as_bytes(),
            Front::Elsewhere,
            Some(&session),
            baud_domain::Row::ZERO,
        );
        let script = crate::ssh::quoted(std::ffi::OsStr::new(crate::ssh::SCRIPT))
            .expect("the script quotes")
            .into_string()
            .expect("UTF-8");
        let want: Vec<String> = [
            "ssh",
            "-p",
            "2222",
            "-i",
            "k",
            "-t",
            "demo",
            "--",
            "exec",
            "/bin/sh",
            "-c",
            script.as_str(),
            "sh",
            "'demo.example'",
            "'hx'",
            "'/home/u/it'\\''s $x.md'",
        ]
        .iter()
        .map(|w| w.to_string())
        .collect();
        assert_eq!(window, Ok(Ok(want)), "the window's argv");
        assert_eq!(
            remote_click(
                rules_of!(PATH_WINDOW),
                link.as_bytes(),
                Front::Local,
                Some(&session),
                baud_domain::Row::ZERO
            ),
            Err(crate::link::Refused::Host),
            "the control: printed by the shell"
        );
    }

    /// **A remote program's `file://` link that names no machine of the
    /// session's opens nothing, and says why**: `localhost`, this machine's
    /// name and the empty host, in either spelling, name this machine, which the session's ssh
    /// does not reach, and with no session printing it there is nothing to
    /// open it through; the bar says each in words, by the session that
    /// printed it. The control: the
    /// destination as typed opens. Named breaks: skip `ssh::names` --
    /// `localhost` opens on the far end; open with no session -- there is
    /// no argv to build, and a click must not fall back to this machine.
    #[test]
    fn a_remote_link_naming_another_machine_opens_nothing_and_says_why() {
        use crate::link::{Refused, refusal_words};
        let session = demo_session();
        let click = |uri: &str, session| {
            remote_click(
                rules_of!(PATH_WINDOW),
                marked(uri, "it").as_bytes(),
                Front::Elsewhere,
                session,
                baud_domain::Row::ZERO,
            )
        };
        for uri in [
            "file://localhost/etc/hosts",
            "file://here.example/etc/hosts",
            "file:///etc/hosts",
            "file:/etc/hosts",
        ] {
            assert_eq!(
                click(uri, Some(&session)),
                Ok(Err(Refused::Elsewhere)),
                "{uri}"
            );
        }
        assert_eq!(
            click("file://demo/etc/hosts", None),
            Ok(Err(Refused::NoSession)),
            "no session printed it"
        );
        assert!(
            matches!(click("file://demo/etc/hosts", Some(&session)), Ok(Ok(_))),
            "the control: the destination as typed"
        );
        for why in [Refused::Elsewhere, Refused::NoSession] {
            assert!(
                refusal_words(why).contains("ssh session")
                    && refusal_words(why).contains("printed it"),
                "{why:?}: {}",
                refusal_words(why)
            );
        }
    }

    /// **A remote link's window runs `remote` on the other machine, or
    /// `command` as written, and a program under `~/` there is under that
    /// machine's home folder**: the click's argv carries `~/` outside the
    /// quotes, never this machine's home, and `remote`, when given, in
    /// place of `command`. Named breaks: build the far program from the
    /// local window's argv -- this machine's home reaches the other one;
    /// ignore `remote` -- `hx` runs there.
    #[test]
    fn a_remote_links_window_runs_remote_with_the_far_home() {
        let session = demo_session();
        let link = marked("file://demo/srv/a.md", "it");
        let program = |config: &str| {
            let argv = remote_click(
                rules_of!(config),
                link.as_bytes(),
                Front::Elsewhere,
                Some(&session),
                baud_domain::Row::ZERO,
            )
            .expect("a remote link")
            .expect("a window");
            argv[argv.len() - 2..].to_vec()
        };
        let home = "[link.path]\naction = \"window\"\ncommand = [\"~/bin/hx\", \"{path}\"]\n";
        assert_eq!(program(home), ["~/'bin/hx'", "'/srv/a.md'"]);
        let remote = format!("{home}remote = [\"~/bin/vi\", \"{{path}}\"]\n");
        assert_eq!(program(&remote), ["~/'bin/vi'", "'/srv/a.md'"]);
        assert_eq!(
            program(&format!("{PATH_WINDOW}remote = [\"vi\", \"{{path}}\"]\n")),
            ["'vi'", "'/srv/a.md'"],
            "the control: a name on the far PATH"
        );
    }

    /// **An `OSC 8` `file://` link and the same URI printed as plain text
    /// open the same window**, `[link.path]`'s, with its path decoded; a
    /// `[link.file]` rule of yours keeps its own answer, and a remote
    /// program's link it takes is refused as before. Named breaks: take only
    /// marked links remotely -- the plain one is refused; take a remote link
    /// past a `[link.file]` rule -- it opens.
    #[test]
    fn a_marked_and_a_plain_file_link_take_the_same_window() {
        let session = demo_session();
        let uri = "file://demo/srv/a%20b.md";
        let bytes = format!("{}\r\n{uri}\r\n", marked(uri, "ab"));
        let click = |config: &str, row| {
            remote_click(
                rules_of!(config),
                bytes.as_bytes(),
                Front::Elsewhere,
                Some(&session),
                baud_domain::Row::from_index(row),
            )
        };
        let marked_one = click(PATH_WINDOW, 0);
        assert!(
            matches!(&marked_one, Ok(Ok(argv)) if argv.last().map(String::as_str) == Some("'/srv/a b.md'")),
            "{marked_one:?}"
        );
        assert_eq!(click(PATH_WINDOW, 1), marked_one, "the plain one");
        let ruled = format!(
            "{PATH_WINDOW}\n[link.file]\naction = \"window\"\ncommand = [\"hx\", \"{{url}}\"]\n"
        );
        for row in [0, 1] {
            assert_eq!(
                click(&ruled, row),
                Err(crate::link::Refused::Host),
                "row {row}: [link.file] keeps its answer"
            );
        }
    }

    /// **A remote program's `file://` link a click opens on the session's
    /// machine looks as that click acts**: under `[link.path] action =
    /// "window"`, marked with `OSC 8` or written as plain text and named by
    /// the destination as typed or as `ssh -G` resolved it, it draws its
    /// line at rest and its preview names the window, `In a window on demo`,
    /// not "Won't open". The control: the same link with no window
    /// configured is refused by its click, draws no line and says it won't
    /// open. Named breaks: judge the look with `admit_link` alone
    /// (`link::verdict`, `Links::marked`, `Uris::read_marking`) -- the
    /// preview says "Won't open" and no line is drawn while the click
    /// opens; label an opening remote link as any other -- the preview names
    /// no window.
    #[test]
    fn a_remote_link_the_click_opens_looks_and_previews_as_its_window() {
        use crate::lined::Mark;
        let session = demo_session();
        for host in ["demo", "demo.example"] {
            let uri = format!("file://{host}/srv/notes.md");
            for (form, bytes) in [
                ("OSC 8", format!(" {}", marked(&uri, "notes"))),
                ("plain", format!(" {uri}")),
            ] {
                let row = baud_domain::Row::ZERO;
                let (lit, preview, click) = remote_look(
                    rules_of!(PATH_WINDOW),
                    bytes.as_bytes(),
                    Some(&session),
                    row,
                );
                let what = format!("{host}, {form}: {preview:?}");
                assert_eq!(click, Ok(crate::link::Click::Window), "{what}");
                assert_eq!(lit, Some(Mark::Line), "{what}: no line at rest");
                assert!(
                    preview.starts_with("In a window on demo  "),
                    "{what}: the preview names no window"
                );
                let (lit, preview, click) =
                    remote_look(rules_of!(""), bytes.as_bytes(), Some(&session), row);
                assert_eq!(
                    (lit, preview.starts_with("Won't open"), click.is_err()),
                    (None, true, true),
                    "the control, no window configured: {what}"
                );
            }
        }
    }

    /// **A remote program's `file://` link the click opens nothing for says
    /// why, and draws no line**: with no session in front, naming this
    /// machine (`localhost`, its name, the empty host), and with a path
    /// that cannot reach the far end whole (a decoded control), the click
    /// is refused and the preview gives that refusal in words, for either
    /// form. The control: the destination as typed opens. Named break:
    /// draw a remote link at rest whatever its window comes to -- a refused
    /// one is lined.
    #[test]
    fn a_remote_link_the_click_refuses_says_why_in_its_preview() {
        use crate::lined::Mark;
        use crate::link::{Refused, refusal_words};
        let session = demo_session();
        for (uri, session, why) in [
            ("file://demo/srv/a.md", None, Some(Refused::NoSession)),
            (
                "file://localhost/etc/hosts",
                Some(&session),
                Some(Refused::Elsewhere),
            ),
            (
                "file://here.example/etc/hosts",
                Some(&session),
                Some(Refused::Elsewhere),
            ),
            (
                "file:///etc/hosts",
                Some(&session),
                Some(Refused::Elsewhere),
            ),
            (
                "file://demo/srv/a%0Ab.md",
                Some(&session),
                Some(Refused::Byte),
            ),
            ("file://demo/srv/a.md", Some(&session), None),
        ] {
            for (form, bytes) in [
                ("OSC 8", format!(" {}", marked(uri, "it"))),
                ("plain", format!(" {uri}")),
            ] {
                let row = baud_domain::Row::ZERO;
                let (lit, preview, click) =
                    remote_look(rules_of!(PATH_WINDOW), bytes.as_bytes(), session, row);
                let what = format!("{uri}, {form}: {preview:?}");
                match why {
                    Some(why) => {
                        assert_eq!(click, Err(why), "{what}");
                        assert_eq!(lit, None, "{what}: a refused link is lined");
                        assert!(
                            preview.starts_with(&format!("Won't open, {}  ", refusal_words(why))),
                            "{what}"
                        );
                    }
                    None => assert_eq!(
                        (lit, preview.starts_with("In a window on demo")),
                        (Some(Mark::Line), true),
                        "the control: {what}"
                    ),
                }
            }
        }
    }

    /// **The session filed over a link's rows moves its look**: rows a
    /// remote program wrote with no session filed draw no line under its
    /// link, and its preview says no one session printed it; the session
    /// filed over them moves the generation the look under the pointer is
    /// kept by, the same one again moves nothing, and the link is lined
    /// and its preview names the window; a worker that stops answering
    /// takes it away. Named breaks: leave the generation where it was when
    /// a session is filed (`file` in place of `file_since`) -- the look
    /// under the pointer is not read again; open through a session after
    /// `Paths::close` -- the preview still names a window.
    #[test]
    fn the_session_filed_over_a_links_rows_moves_its_look() {
        use crate::link::{MachineNames, Refused, refusal_words};
        let kept = std::sync::Arc::new(demo_session());
        let bytes = format!(" {}", marked("file://demo/srv/a.md", "a"));
        on_remote(
            rules_of!(PATH_WINDOW),
            bytes.as_bytes(),
            Front::Elsewhere,
            None,
            baud_domain::Row::ZERO,
            |t, p, under| {
                let mut uris = crate::lined::Uris::default();
                uris.set_names(MachineNames::one(HERE));
                uris.set_rules(p.rules().clone());
                let read = |p: &mut Paths, uris: &mut crate::lined::Uris| {
                    let mut l = Lines::default();
                    let _ = l.scan(
                        t,
                        at(),
                        &mut Links {
                            uris: &mut *uris,
                            urls: Turned::On,
                            paths: &mut *p,
                        },
                    );
                    let printed = p.printed(t, under);
                    let preview = crate::link::preview_text_with(
                        under,
                        t,
                        &MachineNames::one(HERE),
                        baud_domain::Cols::of_index(120),
                        &*p,
                        printed,
                    )
                    .expect("a preview");
                    (!l.lines().is_empty(), preview)
                };
                let no_session = format!("Won't open, {}", refusal_words(Refused::NoSession));
                let (lit, preview) = read(&mut *p, &mut uris);
                assert!(!lit, "lined with no session filed");
                assert!(preview.starts_with(&no_session), "{preview:?}");
                let last = baud_domain::Row::from_index(usize::from(WS.ws_row) - 1);
                let rows = LineSpan::of(
                    at().line(Rows::ZERO, baud_domain::Row::ZERO),
                    at().line(Rows::ZERO, last),
                );
                let elsewhere = Seen::Named(Front::Elsewhere);
                let was = p.generation();
                p.written_by(elsewhere, Some(&kept), rows);
                assert_ne!(p.generation(), was, "the generation moves");
                let was = p.generation();
                p.written_by(elsewhere, Some(&kept), rows);
                assert_eq!(p.generation(), was, "the same session moves nothing");
                let (lit, preview) = read(&mut *p, &mut uris);
                assert!(
                    lit && preview.starts_with("In a window on demo"),
                    "{preview:?}"
                );
                p.close();
                let (lit, preview) = read(&mut *p, &mut uris);
                assert!(!lit, "lined after the worker stopped");
                assert!(preview.starts_with(&no_session), "{preview:?}");
            },
        );
    }

    /// **A link opens through the ssh session that printed it, not the one
    /// in front at the click**: rows printed under `ssh demo`, clicked
    /// while `ssh other` is in front, open a window running `ssh` to
    /// `demo`, which the preview names, and nothing names `other`. The
    /// control: printed under `other` too, the window is `other`'s. Named
    /// break: read the session in front (`Paths::front_session`) in
    /// `PathsAt::session` -- the window runs `ssh` to `other`, whose far
    /// end refuses a host not its own only after the window opened.
    #[test]
    fn a_link_opens_through_the_session_that_printed_it() {
        use crate::link::MachineNames;
        let demo = demo_session();
        let argv: Vec<std::ffi::OsString> = ["ssh", "other"].iter().map(Into::into).collect();
        let other = crate::ssh::session(&argv, |_| Some(b"hostname other.example\n".to_vec()))
            .expect("a session");
        let bytes = format!(" {}", marked("file://demo/srv/a.md", "a"));
        let open = |printed: &crate::ssh::Session| {
            on_printed(
                rules_of!(PATH_WINDOW),
                bytes.as_bytes(),
                Front::Elsewhere,
                (Some(printed), Some(&other)),
                baud_domain::Row::ZERO,
                |t, p, under| {
                    let click =
                        crate::link::click_on(under, t, &MachineNames::one(HERE), p.rules(), &*p);
                    let argv: Vec<String> = match click {
                        crate::link::ClickOn::Remote(Ok(run), _) => run
                            .argv()
                            .iter()
                            .map(|w| w.to_str().expect("UTF-8").to_owned())
                            .collect(),
                        got => panic!("{got:?}"),
                    };
                    let printed = p.printed(t, under);
                    let preview = crate::link::preview_text_with(
                        under,
                        t,
                        &MachineNames::one(HERE),
                        baud_domain::Cols::of_index(120),
                        &*p,
                        printed,
                    )
                    .expect("a preview");
                    (argv, preview)
                },
            )
        };
        let (argv, preview) = open(&demo);
        assert!(argv.iter().any(|w| w == "demo"), "{argv:?}");
        assert!(!argv.iter().any(|w| w == "other"), "{argv:?}");
        assert!(preview.starts_with("In a window on demo"), "{preview:?}");
        // The control: printed under the session in front.
        let (argv, preview) = open(&other);
        assert!(argv.iter().any(|w| w == "other"), "{argv:?}");
        assert!(!argv.iter().any(|w| w == "demo"), "{argv:?}");
        assert!(preview.starts_with("In a window on other"), "{preview:?}");
    }

    /// **A link on rows no one ssh session printed opens nothing remotely,
    /// whatever ssh is in front**: rows a remote program wrote with no
    /// session filed, and rows two sessions printed, refuse the click with
    /// [`crate::link::Refused::NoSession`] while `ssh demo` is in front,
    /// draw no line and preview "Won't open". The control: the same rows
    /// printed under `ssh demo` alone open. Named breaks: fall back to the
    /// session in front for rows with none -- the first clicks open; keep
    /// the first session filed over a row -- the second opens.
    #[test]
    fn a_link_no_one_ssh_session_printed_opens_nothing_remotely() {
        use crate::link::{ClickOn, MachineNames, Refused, click_on, refusal_words};
        let demo = demo_session();
        let argv: Vec<std::ffi::OsString> = ["ssh", "other"].iter().map(Into::into).collect();
        let other = std::sync::Arc::new(
            crate::ssh::session(&argv, |_| Some(b"hostname other.example\n".to_vec()))
                .expect("a session"),
        );
        let bytes = format!(" {}", marked("file://demo/srv/a.md", "a"));
        let no_session = format!("Won't open, {}", refusal_words(Refused::NoSession));
        // Rows `printed` under no session, and with `also` filed over them
        // after: the click, whether the rest look draws a line, and the
        // preview, with `ssh demo` in front.
        let judged = |printed: Option<&crate::ssh::Session>, also: bool| {
            on_printed(
                rules_of!(PATH_WINDOW),
                bytes.as_bytes(),
                Front::Elsewhere,
                (printed, Some(&demo)),
                baud_domain::Row::ZERO,
                |t, p, under| {
                    if also {
                        let last = baud_domain::Row::from_index(usize::from(WS.ws_row) - 1);
                        let rows = LineSpan::of(
                            at().line(Rows::ZERO, baud_domain::Row::ZERO),
                            at().line(Rows::ZERO, last),
                        );
                        p.written_by(Seen::Named(Front::Elsewhere), Some(&other), rows);
                    }
                    let opens = match click_on(under, t, &MachineNames::one(HERE), p.rules(), &*p) {
                        ClickOn::Remote(Ok(_), _) => Ok(()),
                        ClickOn::Remote(Err(why), _) => Err(why),
                        got => panic!("{got:?}"),
                    };
                    let mut uris = crate::lined::Uris::default();
                    uris.set_names(MachineNames::one(HERE));
                    uris.set_rules(p.rules().clone());
                    let mut l = Lines::default();
                    let _ = l.scan(
                        t,
                        at(),
                        &mut Links {
                            uris: &mut uris,
                            urls: Turned::On,
                            paths: &mut *p,
                        },
                    );
                    let printed = p.printed(t, under);
                    let preview = crate::link::preview_text_with(
                        under,
                        t,
                        &MachineNames::one(HERE),
                        baud_domain::Cols::of_index(120),
                        &*p,
                        printed,
                    )
                    .expect("a preview");
                    (opens, !l.lines().is_empty(), preview)
                },
            )
        };
        for (printed, also, what) in [
            (None, false, "no session"),
            (Some(&demo), true, "two sessions"),
        ] {
            let (opens, lit, preview) = judged(printed, also);
            assert_eq!(opens, Err(Refused::NoSession), "{what}");
            assert!(!lit, "{what}: lined");
            assert!(preview.starts_with(&no_session), "{what}: {preview:?}");
        }
        // The control: printed under the session in front alone.
        let (opens, lit, preview) = judged(Some(&demo), false);
        assert_eq!(opens, Ok(()));
        assert!(
            lit && preview.starts_with("In a window on demo"),
            "{preview:?}"
        );
    }

    /// **A row a local program wrote after an ssh session ended is not that
    /// session's**: `ssh demo` clears the screen, so its read covers every
    /// row, and exits; the shell then prints a `file://demo` link on row 2.
    /// Its click is refused with [`crate::link::Refused::NoSession`] and
    /// its preview says "Won't open", while the session's own link on row
    /// 0 still opens on `demo`; so with the shell's rows filed before the
    /// session's. The control: with no local writer on row 2, its link
    /// opens on `demo`. Named break: no printer filed with a local writer
    /// (`Fact::added_to`) -- row 2's click opens a window on `demo`.
    #[test]
    fn a_row_printed_here_after_an_ssh_session_ended_is_not_its() {
        use crate::link::{ClickOn, MachineNames, Refused, click_on, refusal_words};
        use baud_domain::Row;
        /// What is filed over the screen's rows: `ssh demo`'s read over
        /// every row alone; then the shell's over row 2; or the shell's
        /// over row 2 first and `ssh demo`'s over every row after it.
        #[derive(Clone, Copy, Debug)]
        enum Filed {
            Session,
            ThenHere,
            HereFirst,
        }
        let demo = std::sync::Arc::new(demo_session());
        let bytes = format!(
            " {}\r\n\r\n {}",
            marked("file://demo/srv/a.md", "a"),
            marked("file://demo/srv/b.md", "b")
        );
        let no_session = format!("Won't open, {}", refusal_words(Refused::NoSession));
        let line = |r: Row| at().line(Rows::ZERO, r);
        let shells = LineSpan::of(line(Row::from_index(2)), line(Row::from_index(2)));
        let every = LineSpan::of(
            line(Row::ZERO),
            line(Row::from_index(usize::from(WS.ws_row) - 1)),
        );
        // The click on `row`'s link and its preview, with no session in
        // front: the session has ended.
        let judged = |filed: Filed, row: Row| {
            let printed = match filed {
                Filed::Session | Filed::ThenHere => Some(&*demo),
                Filed::HereFirst => None,
            };
            on_printed(
                rules_of!(PATH_WINDOW),
                bytes.as_bytes(),
                Front::Elsewhere,
                (printed, None),
                row,
                |t, p, under| {
                    match filed {
                        Filed::Session => {}
                        Filed::ThenHere => p.written_by(Seen::Named(Front::Local), None, shells),
                        Filed::HereFirst => {
                            p.written_by(Seen::Named(Front::Local), None, shells);
                            p.written_by(Seen::Named(Front::Elsewhere), Some(&demo), every);
                        }
                    }
                    let opens = match click_on(under, t, &MachineNames::one(HERE), p.rules(), &*p) {
                        ClickOn::Remote(Ok(_), _) => Ok(()),
                        ClickOn::Remote(Err(why), _) => Err(why),
                        got => panic!("{got:?}"),
                    };
                    let printed = p.printed(t, under);
                    let preview = crate::link::preview_text_with(
                        under,
                        t,
                        &MachineNames::one(HERE),
                        baud_domain::Cols::of_index(120),
                        &*p,
                        printed,
                    )
                    .expect("a preview");
                    (opens, preview)
                },
            )
        };
        for filed in [Filed::ThenHere, Filed::HereFirst] {
            let (opens, preview) = judged(filed, Row::from_index(2));
            assert_eq!(opens, Err(Refused::NoSession), "{filed:?}");
            assert!(preview.starts_with(&no_session), "{filed:?}: {preview:?}");
            let (opens, preview) = judged(filed, Row::ZERO);
            assert_eq!(opens, Ok(()), "{filed:?}: the session's own row");
            assert!(
                preview.starts_with("In a window on demo"),
                "{filed:?}: {preview:?}"
            );
        }
        // The control: no local writer on row 2.
        let (opens, preview) = judged(Filed::Session, Row::from_index(2));
        assert_eq!(opens, Ok(()));
        assert!(preview.starts_with("In a window on demo"), "{preview:?}");
    }

    /// **A row nothing was filed over is not known, everywhere**: with the
    /// shell's folder reported and this machine named, a `./` path on a row
    /// no read filed is asked about by no one and draws no line, and a plain
    /// `file://` link there is unmarked and its click refused as not named.
    /// The control: the same row filed as a local program's -- the path is
    /// asked about and the link draws its line and opens. Named break: read
    /// no facts as local (`Paths::wrote`).
    #[test]
    fn a_row_nothing_was_filed_over_is_not_known() {
        for filed in [false, true] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            feed(
                &mut t,
                b"\x1b]7;file://here.example/home/u\x07./a.txt file:///etc/hosts",
            );
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            p.set_names(crate::link::MachineNames::of(vec![HERE.to_string()]));
            if filed {
                p.all_written_here();
            }
            let mut u = crate::lined::Uris::default();
            u.set_names(crate::link::MachineNames::one(HERE));
            let mut l = Lines::default();
            let _ = p.observe(&t, at());
            p.begin_collect();
            let _ = l.scan(
                &t,
                at(),
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: &mut p,
                },
            );
            let asked: Vec<Vec<u8>> = p.end_collect().into_iter().map(|c| c.path).collect();
            let spot = crate::link::Spot {
                scrolled: at(),
                back: Rows::ZERO,
                row: baud_domain::Row::ZERO,
                col: baud_domain::Col::at_index(10),
            };
            let under = crate::link::hovered(&t, spot).expect("the link");
            let click = crate::link::clicked(
                &under,
                &t,
                &crate::link::MachineNames::one(HERE),
                p.rules(),
                &p,
            );
            let path = b"/home/u/a.txt".to_vec();
            match filed {
                false => {
                    assert_eq!(asked, Vec::<Vec<u8>>::new(), "the path asked about");
                    assert_eq!(runs(&l), [], "no line");
                    assert_eq!(click, Err(crate::link::Refused::Unnamed), "the click");
                }
                true => {
                    assert_eq!(asked, [path], "the control: the path asked about");
                    assert_eq!(runs(&l), [(0, 8, 24)], "the control: the link's line");
                    assert_eq!(
                        click,
                        Ok(crate::link::Click::Reveal),
                        "the control: the click"
                    );
                }
            }
        }
    }

    /// One fact filed over lines `a` to `b` of the window, for the property
    /// below: a writer named local, a multiplexer, remote, one not named,
    /// or the shell's local command.
    #[derive(Clone, Copy, Debug)]
    struct Filing {
        fact: Fact,
        first: baud_domain::Row,
        last: baud_domain::Row,
    }

    /// The per-line answers of `filings` filed in the order given.
    fn filed(filings: &[Filing]) -> Vec<Option<Provenance>> {
        let mut p = Paths::new(None, Forms::DEFAULT);
        let l = |n: usize| at().line(Rows::ZERO, baud_domain::Row::from_index(n));
        let on = |r| at().line(Rows::ZERO, r);
        for f in filings {
            let _ = p.file(f.fact, LineSpan::of(on(f.first), on(f.last)));
        }
        (0..8).map(|n| p.wrote(Surface::Primary, l(n))).collect()
    }

    /// One event of the property below: a fact filed, or lines erased whole
    /// as the core's record names them.
    #[derive(Clone, Copy, Debug)]
    enum Event {
        File(Filing),
        Erase(baud_domain::Row, baud_domain::Row),
    }

    /// The per-line answers of `events` in the order given, each erasure
    /// forgotten as a read's record has it forgotten
    /// ([`Paths::forget_erased`]).
    fn happened(events: &[Event]) -> Vec<Option<Provenance>> {
        let mut p = Paths::new(None, Forms::DEFAULT);
        let l = |n: usize| at().line(Rows::ZERO, baud_domain::Row::from_index(n));
        let on = |r| at().line(Rows::ZERO, r);
        for e in events {
            match *e {
                Event::File(f) => {
                    let _ = p.file(f.fact, LineSpan::of(on(f.first), on(f.last)));
                }
                Event::Erase(a, b) => p.forget_erased(Erased(LineSpan::of(on(a), on(b)))),
            }
        }
        (0..8).map(|n| p.wrote(Surface::Primary, l(n))).collect()
    }

    /// `events` with the facts between each two erasures put in another
    /// order by `turn`, each erasure where it was.
    fn reordered(events: &[Event], turn: impl Fn(&mut Vec<Event>)) -> Vec<Event> {
        let (mut out, mut run) = (Vec::new(), Vec::new());
        for &e in events {
            match e {
                Event::File(_) => run.push(e),
                Event::Erase(..) => {
                    turn(&mut run);
                    out.append(&mut run);
                    out.push(e);
                }
            }
        }
        turn(&mut run);
        out.append(&mut run);
        out
    }

    /// **R5: what wrote a line is one total function of the facts filed
    /// over it since it was last erased, whatever order they came in**:
    /// over 20,000 random sequences of up to eight events on eight lines --
    /// writers named local, multiplexer and remote, writers not named,
    /// reads nothing names, the shell's local commands, and lines erased
    /// whole -- the answer for every line is the same with the facts
    /// between each two erasures filed in order, reversed and rotated; it
    /// is the answer the facts over the line since its last erasure give
    /// alone, on a store of their own; a line any remote writer wrote since
    /// is remote; a line is local only if no writer named over it since is
    /// other than local, every writer not named over it since is under the
    /// shell's local command, and no read nothing names wrote it since; and
    /// a line with no fact since has no answer. Named breaks: the last
    /// writer filed wins -- the orders disagree; facts kept across an
    /// erasure (`Paths::forget_erased` doing nothing) -- a line answers
    /// for text erased.
    #[test]
    fn a_lines_writer_is_one_function_of_its_facts_in_any_order() {
        let facts = [
            Fact::Writer(Writer::Local),
            Fact::Writer(Writer::Multiplexed),
            Fact::Writer(Writer::Remote),
            Fact::NotNamed,
            Fact::Nameless,
            Fact::Here,
        ];
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            usize::try_from(seed % u64::try_from(n).expect("small")).expect("small")
        };
        let row = baud_domain::Row::from_index;
        for _ in 0..20_000 {
            let len = 1 + next(8);
            let events: Vec<Event> = (0..len)
                .map(|_| {
                    let (a, b) = (next(8), next(8));
                    let (first, last) = (row(a.min(b)), row(a.max(b)));
                    match next(4) {
                        0 => Event::Erase(first, last),
                        _ => Event::File(Filing {
                            fact: facts[next(facts.len())],
                            first,
                            last,
                        }),
                    }
                })
                .collect();
            let forward = happened(&events);
            let back = reordered(&events, |r| r.reverse());
            let turned = reordered(&events, |r| {
                let k = r.len() / 2;
                r.rotate_left(k);
            });
            assert_eq!(happened(&back), forward, "reversed: {events:?}");
            assert_eq!(happened(&turned), forward, "rotated: {events:?}");
            for (n, got) in forward.iter().enumerate() {
                let mut over: Vec<Fact> = Vec::new();
                for e in &events {
                    match *e {
                        Event::Erase(a, b) if (a.index()..=b.index()).contains(&n) => over.clear(),
                        Event::File(f) if (f.first.index()..=f.last.index()).contains(&n) => {
                            over.push(f.fact);
                        }
                        Event::Erase(..) | Event::File(_) => {}
                    }
                }
                let since: Vec<Filing> = over
                    .iter()
                    .map(|&fact| Filing {
                        fact,
                        first: row(n),
                        last: row(n),
                    })
                    .collect();
                let has = |f: Fact| over.contains(&f);
                let what = format!("line {n}: {over:?} since its last erasure gave {got:?}");
                assert_eq!(filed(&since)[n], *got, "alone: {what}");
                if has(Fact::Writer(Writer::Remote)) {
                    assert_eq!(*got, Some(Provenance::Remote), "{what}");
                }
                if *got == Some(Provenance::Local) {
                    assert!(!has(Fact::Writer(Writer::Multiplexed)), "{what}");
                    assert!(!has(Fact::Writer(Writer::Remote)), "{what}");
                    assert!(!has(Fact::NotNamed) || has(Fact::Here), "{what}");
                    assert!(!has(Fact::Nameless), "{what}");
                }
                let named = has(Fact::Writer(Writer::Local))
                    || has(Fact::Writer(Writer::Multiplexed))
                    || has(Fact::Writer(Writer::Remote))
                    || has(Fact::NotNamed)
                    || has(Fact::Nameless);
                if !named {
                    assert_eq!(*got, None, "{what}");
                }
            }
        }
    }

    /// **A run a part's lines cannot place is left out and counted, and a
    /// record that held more erasures than it has room for names none**
    /// (`Erased::of`, E1): `ED 2` read where no line of the primary is
    /// placed names no run and one not placed; placed, one run and none
    /// not placed. Five `EL 2` apart overflow the record. Named break: a
    /// run not placed dropped uncounted -- the count reads 0.
    #[test]
    fn an_erasure_not_placed_is_counted_and_an_overflow_names_none() {
        let ws = WinSize {
            ws_col: 60,
            ws_row: 10,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let p = Paths::new(None, Forms::DEFAULT);
        feed(&mut t, b"x\x1b[2J");
        let rec = t.take_record();
        let none = CoreLines::of(None).part(None);
        assert_eq!(
            Erased::of(&rec, &none, &p, Screen::Normal),
            ErasedLines::Placed(Vec::new(), 1),
            "not placed"
        );
        let first = baud_domain::LineIdx::FIRST;
        let placed = CoreLines::of(Some((first, at()))).part(Some(first));
        assert!(
            matches!(
                Erased::of(&rec, &placed, &p, Screen::Normal),
                ErasedLines::Placed(ref e, 0) if e.len() == 1
            ),
            "the control: placed"
        );
        feed(
            &mut t,
            b"\x1b[1;1H\x1b[2K\x1b[3;1H\x1b[2K\x1b[5;1H\x1b[2K\x1b[7;1H\x1b[2K\x1b[9;1H\x1b[2K",
        );
        let five = t.take_record();
        assert_eq!(
            Erased::of(&five, &placed, &p, Screen::Normal),
            ErasedLines::Overflowed
        );
    }

    /// **An erasure of lines with no fact moves nothing**
    /// (`Store::forget_lines`, `Paths::forget_erased`): lines 0 and 1 filed
    /// remote; an erasure of lines 3 to 5, which hold no fact, as the blank
    /// lines an `EL`-heavy redraw erases again hold none, returns
    /// `Moved::No` and leaves the generation, the origins and the lines to
    /// read again as they were: no row is read again for it. The control:
    /// an erasure of line 1 moves the generation and the origins, and line
    /// 1 is not known. Named break: refile whatever a forget found
    /// (`Paths::forget_erased` without its `Moved::Yes` test) -- the
    /// generation moves.
    #[test]
    fn an_erasure_of_lines_with_no_fact_moves_nothing() {
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        let at = |n: usize| AbsLine::at(Rows::of_count(n));
        p.written_remote(LineSpan::of(at(0), at(1)));
        let was = (p.generation, p.origins, p.refiled);
        let blank = LineSpan::of(at(3), at(5));
        assert_eq!(p.primary.forget_lines(blank), Moved::No);
        p.forget_erased(Erased(blank));
        assert_eq!((p.generation, p.origins, p.refiled), was);
        p.forget_erased(Erased(LineSpan::of(at(1), at(1))));
        assert!(p.generation != was.0 && p.origins != was.1, "the control");
        assert_eq!(p.origin(at(1)), Origin::Unknown, "the control");
    }

    /// **A forget that cuts one run in two keeps both pieces**
    /// (`Store::forget_lines`): lines 0 to 9 filed remote, one run; an
    /// erasure of lines 4 and 5 leaves two runs, lines 0 to 3 and 6 to 9,
    /// each remote, and lines 4 and 5 not known. The control: an erasure
    /// over the top of the upper piece, lines 0 and 1, leaves its lines 2
    /// and 3, and the lower piece as it was. Named break: the piece below
    /// the span never kept (`forget_lines`'s `below`) -- line 6 is not
    /// known.
    #[test]
    fn a_forget_that_cuts_one_run_in_two_keeps_both_pieces() {
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        let at = |n: usize| AbsLine::at(Rows::of_count(n));
        let spans = |p: &Paths| p.primary.runs.iter().map(|&(s, _)| s).collect::<Vec<_>>();
        p.written_remote(LineSpan::of(at(0), at(9)));
        assert_eq!(spans(&p), [LineSpan::of(at(0), at(9))], "one run");
        p.forget_erased(Erased(LineSpan::of(at(4), at(5))));
        assert_eq!(
            spans(&p),
            [LineSpan::of(at(0), at(3)), LineSpan::of(at(6), at(9))],
            "both pieces"
        );
        assert_eq!(p.origin(at(3)), Origin::Remote, "above");
        assert_eq!(p.origin(at(6)), Origin::Remote, "below");
        assert_eq!(p.origin(at(4)), Origin::Unknown);
        assert_eq!(p.origin(at(5)), Origin::Unknown);
        p.forget_erased(Erased(LineSpan::of(at(0), at(1))));
        assert_eq!(
            spans(&p),
            [LineSpan::of(at(2), at(3)), LineSpan::of(at(6), at(9))],
            "the control"
        );
    }

    /// **A row's facts are found by a binary search, however long the
    /// session**: with 10,000 runs, writers alternating so none merge, the
    /// 50 rows of a window are looked up, for their origin and for a link
    /// read across them, in at most 15 steps a lookup (log2 of
    /// 10,000 is 13.3), where a walk of every run took 10,000. Named break:
    /// find the run by walking the list.
    #[test]
    fn a_long_sessions_rows_are_looked_up_in_log_steps() {
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        let l = |n: usize| at().line(Rows::ZERO, baud_domain::Row::from_index(n));
        for n in 0..10_000 {
            let front = match n % 2 {
                0 => Front::Local,
                _ => Front::Elsewhere,
            };
            p.written_under(Seen::Named(front), LineSpan::of(l(n), l(n)));
        }
        assert_eq!(p.primary.runs.len(), 10_000, "none merged");
        let before = p.lookups().value();
        for n in 9_950..10_000 {
            let _ = p.origin(l(n));
            let _ = p.origin_over(Surface::Primary, (l(n), l(n)));
        }
        let steps = p.lookups().value() - before;
        assert!(steps <= 2 * 50 * 15, "{steps} steps for 100 lookups");
        assert!(steps >= 2 * 50, "every lookup counted: {steps}");
    }

    /// **Spans of one origin that touch are one, and a span gone from
    /// history goes**: adjacent local reads merge, a remote one between
    /// stays its own, and trimming at a line drops only what ends above it.
    /// Named break: never trim -- the dropped span still answers.
    #[test]
    fn line_spans_merge_and_leave_with_history() {
        let mut p = Paths::new(None, Forms::DEFAULT);
        let l = |n: usize| at().line(Rows::ZERO, baud_domain::Row::from_index(n));
        p.written_under(Seen::Named(Front::Local), LineSpan::of(l(0), l(0)));
        p.written_under(Seen::Named(Front::Local), LineSpan::of(l(1), l(2)));
        p.written_under(Seen::Named(Front::Elsewhere), LineSpan::of(l(3), l(3)));
        p.written_under(Seen::Named(Front::Local), LineSpan::of(l(3), l(3)));
        assert_eq!(
            p.primary.runs.len(),
            2,
            "the two local reads merged, the line both wrote apart: {:?}",
            p.primary.runs
        );
        assert_eq!(
            p.origin(l(3)),
            Origin::Remote,
            "a line both wrote is remote"
        );
        p.trim_spans(Screen::Normal, l(3));
        assert_eq!(p.origin(l(1)), Origin::Unknown, "gone with its history");
        assert_eq!(p.origin(l(3)), Origin::Remote);
    }

    /// A window of `WS` with twenty lines of 40 columns fed, so its
    /// history holds fourteen, the record taken, and the count that
    /// numbers its lines.
    fn with_history(t: &mut Term<'_>) -> Scrolled {
        for n in 0..20 {
            feed(t, format!("line {n:02} {}\r\n", "x".repeat(32)).as_bytes());
        }
        let _ = t.take_record();
        Scrolled::START.advanced(Rows::of_count(t.scrollback().len()))
    }

    /// **A window resize, as both drivers make it**: the last line read,
    /// the core resized, the count moved by what it retired or took back,
    /// as `keep::kept_across_resize` moves it, then [`Paths::resized`]. The
    /// count after.
    fn resize_as_drivers_do(
        t: &mut Term<'_>,
        p: &mut Paths,
        scrolled: Scrolled,
        ws: &WinSize,
    ) -> Scrolled {
        let mark = t.history_mark();
        let was_last = last_line(t, scrolled);
        let _ = t.resize(ws);
        let scrolled = match t.history_moved_since(mark) {
            baud_core::Moved::Retired(n) => scrolled.advanced(n),
            baud_core::Moved::Restored(n) => scrolled.retreated(n).expect("within the count"),
        };
        let rec = t.take_record();
        p.resized(&rec, t, was_last, scrolled, Scrolled::START);
        scrolled
    }

    /// Every line from the oldest kept to the window's last.
    fn every_line(t: &Term<'_>, scrolled: Scrolled) -> LineSpan {
        let back = Rows::of_count(t.scrollback().len());
        LineSpan::of(
            scrolled.line(back, baud_domain::Row::ZERO),
            last_line(t, scrolled),
        )
    }

    /// **A span still to be named goes where its text went** (B10,
    /// `Paths::carried_span`), on the core's records of a six-row screen:
    /// a region of rows 2 to 4 scrolled up by one carries a span on row 3
    /// to row 2, exactly; a span from row 1 to row 3, part of it in the
    /// region, takes the hull of where it was and where its moved part
    /// went; a span on row 5, outside it, stays; the same region scrolled
    /// up and then down, which the record keeps as a move within the
    /// band, widens a span on row 3 to the band; and a span on the
    /// alternate screen is not carried by the primary's moves. Named
    /// breaks: shift the whole span by an exact move -- the part outside
    /// the region moves too; leave a span inside a band where it was --
    /// the band's other rows are dropped.
    #[test]
    fn a_span_still_to_be_named_goes_where_its_text_went() {
        let ws = WinSize {
            ws_col: 20,
            ws_row: 6,
        };
        let at = |n: usize| AbsLine::at(Rows::of_count(n));
        let span = |a, b| LineSpan::on(Surface::Primary, at(a), at(b));
        let carried = |bytes: &[u8], s: LineSpan| {
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            let _ = t.take_record();
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let rec = t.take_record();
            let lines = CoreLines::of(Some((t.history_mark().line(), Scrolled::START)))
                .part(Some(t.history_mark().line()));
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            p.carried_span(&rec, &lines, &[Screen::Normal, Screen::Alternate], s)
        };
        let up = b"\x1b[3;5r\x1b[5;1H\n";
        assert_eq!(carried(up, span(3, 3)), span(2, 2), "exact");
        assert_eq!(carried(up, span(1, 3)), span(1, 3), "part in the region");
        assert_eq!(carried(up, span(5, 5)), span(5, 5), "outside it");
        let both = b"\x1b[3;5r\x1b[5;1H\n\x1b[3;1H\x1bM";
        assert_eq!(carried(both, span(3, 3)), span(2, 4), "within the band");
        let p = Paths::new(None, Forms::DEFAULT);
        let alt = LineSpan::on(p.surface(Screen::Alternate), at(3), at(3));
        assert_eq!(carried(up, alt), alt, "another surface");
    }

    /// **A copy leaves a span still to be named where its text is, and
    /// holds the line it wrote under it** (B10, `Paths::carried_span`):
    /// `DECCRA` copies "a rectangular area of characters from one section
    /// to another" (`[[source: vt510-deccra]]`), so the lines it reads keep
    /// their text and each line it writes keeps its own outside the
    /// rectangle. On a six-row screen it copies row 3 from its second
    /// column two rows down, to row 5: a span on row 3 stays on row 3,
    /// where the answer finds the text it was asked about; a span from row
    /// 2 to row 4, the source within it, stays as it was; and row 5, which
    /// holds a copy of part of that text beside its own, is held under the
    /// span on row 3 (`Paths::carried`), so the answer reaches it too. The
    /// control: `IL` on row 3 replaces whole lines and carries the span on
    /// row 3 to row 4, exactly. Named break: a move that replaced part of
    /// each line it wrote carried as one that replaced them whole -- the
    /// span leaves row 3 for row 5, and the answer never reaches row 3.
    #[test]
    fn a_copy_leaves_a_span_still_to_be_named_where_its_text_is() {
        let ws = WinSize {
            ws_col: 20,
            ws_row: 6,
        };
        let at = |n: usize| AbsLine::at(Rows::of_count(n));
        let span = |a, b| LineSpan::on(Surface::Primary, at(a), at(b));
        let carried = |bytes: &[u8], s: LineSpan| {
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            let _ = t.take_record();
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let rec = t.take_record();
            let lines = CoreLines::of(Some((t.history_mark().line(), Scrolled::START)))
                .part(Some(t.history_mark().line()));
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            p.set_pending([s].into_iter(), core::iter::empty());
            let screens = [Screen::Normal, Screen::Alternate];
            let went = p.carried_span(&rec, &lines, &screens, s);
            (went, p.carried(&rec, &lines, &screens))
        };
        let copy = b"\x1b[4;2;4;20;1;6;2;1$v";
        assert_eq!(carried(copy, span(3, 3)).0, span(3, 3), "the copy's source");
        assert_eq!(carried(copy, span(2, 4)).0, span(2, 4), "a span holding it");
        assert_eq!(
            carried(copy, span(3, 3)).1,
            [(span(5, 5), span(3, 3))],
            "the line the copy wrote, held"
        );
        assert_eq!(carried(b"\x1b[4H\x1b[L", span(3, 3)).0, span(4, 4), "IL");
    }

    /// **A part the core says renumbered lines leaves each surface it
    /// names not known, up to the line it is given** (N2): both surfaces
    /// filed local at lines 2 and 9, then a re-cut (`Resize`) through line
    /// 5 of each, then the same local facts again. The primary's line 2 is
    /// not known and its line 9, past the renumbering, local; the
    /// alternate screen keeps both. A core that recorded nothing
    /// (`Unrecorded`) leaves the alternate screen's line 2 not known too.
    /// Named break: renumber the primary alone for `Unrecorded` -- the
    /// alternate screen's line 2 stays local.
    #[test]
    fn a_renumbering_part_darkens_the_surfaces_it_names() {
        use baud_core::Renumbered;
        let at = |n: usize| AbsLine::at(Rows::of_count(n));
        for (why, alternate) in [
            (Renumbered::Resize, Origin::Local),
            (Renumbered::Unrecorded, Origin::Unknown),
        ] {
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            let surfaces = [Surface::Primary, p.surface(Screen::Alternate)];
            let local = |p: &mut Paths| {
                for s in surfaces {
                    for n in [2, 9] {
                        p.written_by_shell(LineSpan::on(s, at(n), at(n)));
                    }
                }
            };
            local(&mut p);
            p.part_renumbered(why, (at(5), at(5)));
            local(&mut p);
            let origins = surfaces.map(|s| (p.origin_at(s, at(2)), p.origin_at(s, at(9))));
            assert_eq!(
                origins,
                [(Origin::Unknown, Origin::Local), (alternate, Origin::Local)],
                "{why:?}"
            );
        }
    }

    /// **A resize that re-cuts the lines darkens them at once, with no
    /// read after it**: history and window filed local, a change of
    /// columns, and nothing fed -- the lookup a hover or a click makes is
    /// not known on every line, history included, where the old facts
    /// would sit on re-cut text. The control: a change of height alone
    /// re-cuts nothing, and the lines stay lit. Named break: file the
    /// renumbering with the next read's part rather than at the resize
    /// (`Paths::resized` does nothing) -- the lines stay local.
    #[test]
    fn a_resize_that_re_cuts_darkens_every_line_with_no_read() {
        let narrow = WinSize {
            ws_col: 30,
            ws_row: 6,
        };
        let shorter = WinSize {
            ws_col: 60,
            ws_row: 5,
        };
        for (ws, recut) in [(&narrow, true), (&shorter, false)] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let scrolled = with_history(&mut t);
            let mut p = Paths::new(None, Forms::DEFAULT);
            let all = every_line(&t, scrolled);
            p.written_by_shell(all);
            assert_eq!(
                p.origin_over(Surface::Primary, (all.first, all.last)),
                Origin::Local
            );
            let scrolled = resize_as_drivers_do(&mut t, &mut p, scrolled, ws);
            if !recut {
                let history = scrolled.line(Rows::of_count(3), baud_domain::Row::ZERO);
                assert_eq!(p.origin(history), Origin::Local, "a history line");
                assert_eq!(p.origin(last_line(&t, scrolled)), Origin::Local, "the last");
                continue;
            }
            // Every number the old facts were filed on, now on re-cut text.
            let mut line = all.first;
            while line <= all.last {
                assert_eq!(p.origin(line), Origin::Unknown, "{line:?} of {all:?}");
                line = line.below();
            }
            let now = every_line(&t, scrolled);
            assert_eq!(
                p.origin_over(Surface::Primary, (now.first, now.last)),
                Origin::Unknown
            );
        }
    }

    /// **A local write after a re-cut does not light a line the re-cut
    /// moved**: a remote line in history, a change of columns, then the
    /// shell's own filing over every line there is -- the prompt redrawn,
    /// written on the numbers the remote text shifted onto -- and every
    /// one of them stays not known. A line past them, which the re-cut
    /// never numbered, is lit by it. Named break: drop the old facts with
    /// no floor after them -- the local filing lights the remote text.
    #[test]
    fn a_local_write_after_a_re_cut_does_not_light_a_moved_line() {
        let narrow = WinSize {
            ws_col: 30,
            ws_row: 6,
        };
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let scrolled = with_history(&mut t);
        let mut p = Paths::new(None, Forms::DEFAULT);
        let all = every_line(&t, scrolled);
        p.written_by_shell(all);
        let remote = scrolled.line(Rows::of_count(8), baud_domain::Row::ZERO);
        p.written_remote(LineSpan::of(remote, remote));
        let scrolled = resize_as_drivers_do(&mut t, &mut p, scrolled, &narrow);
        let now = every_line(&t, scrolled);
        p.written_by_shell(now);
        let mut line = now.first;
        while line <= now.last {
            assert_ne!(p.origin(line), Origin::Local, "{line:?} of {now:?}");
            line = line.below();
        }
        let later = now.last.below();
        p.written_by_shell(LineSpan::of(later, later));
        assert_eq!(
            p.origin(later),
            Origin::Local,
            "a line the re-cut never numbered"
        );
    }

    /// **A re-cut carries the line a folder is read from** (`Dir::from`):
    /// a line of thirty columns, `./x` under it, then the shell's folder
    /// report with the cursor on the line after, at forty columns; the
    /// window narrows to twenty, the long line takes two rows, and `./x`
    /// moves down to the line the folder was read from before. It stays
    /// before that line, which moved with the cursor's: `./x`, printed
    /// before the report, is not read against the folder. The control: the
    /// folder is read from the cursor's line, after `./x`'s. Named break:
    /// the line left where it was (`Paths::reflowed` carrying no
    /// `Dir::from`) -- it is `./x`'s new line.
    #[test]
    fn a_re_cut_carries_the_line_a_folder_is_read_from() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.set_names(crate::link::MachineNames::of(vec![
            "here.example".to_string(),
        ]));
        let s = Scrolled::START;
        let base = t.history_mark().line();
        let _ = p.observe(&t, s);
        let text = format!(
            "{}\r\n./x\r\n\x1b]7;file://here.example/w\x07",
            "y".repeat(30)
        );
        feed(&mut t, text.as_bytes());
        let _ = p.observe(&t, s);
        let line = |n| AbsLine::at(Rows::of_count(n));
        assert_eq!(p.dir.from, Some(line(2)), "the control: the cursor's line");
        let mut recuts = Recuts::default();
        let _ = recuts.resize(
            &mut t,
            &WinSize {
                ws_col: 20,
                ws_row: 6,
            },
        );
        let lines = CoreLines::of(Some((base, s))).on(Rows::of_count(6), s, Numbers::Followed);
        let at = lines.part(Some(t.history_mark().line()));
        let map = recuts.last().expect("the resize, told").map;
        let _ = p.reflowed(map, &at, line(5));
        let x = (0..6)
            .find(|&y| {
                t.row(baud_domain::Row::from_index(y)).is_some_and(|r| {
                    r.first()
                        .and_then(|c| c.content.as_codepoint())
                        .map(|c| c.value())
                        == Some(u32::from(b'.'))
                })
            })
            .expect("./x on the screen");
        assert_eq!(x, 2, "the control: ./x moved down a line");
        assert_eq!(p.dir.from, Some(line(3)), "after ./x");
    }

    /// **Both drivers file a window resize at the resize, by what it told**:
    /// in each, the primary's last line, by its own count, is read before
    /// the resize, the resize is made through the pace's buffer, which
    /// keeps what its re-cut told (`Recuts::resize`, reused), and
    /// `Probes::resized` is called after it, in the same function, with
    /// that resize's own map -- so no hover or frame between reads the old
    /// facts, and no map is read for a resize that told none. The drivers'
    /// resizes are AppKit's and Wayland's, so the source is what can fail.
    /// Named breaks: drop the call from one driver; resize it by
    /// `Term::resize`, which tells nothing.
    #[test]
    fn both_drivers_file_a_resize_at_the_resize() {
        for (name, src, call, resized, told, primary) in [
            (
                "app.rs",
                crate::app_source::app_source(),
                concat!("self.app.pace.recuts.", "resize(&mut self.app.term, &ws)"),
                concat!("self.app.probes.", "resized("),
                concat!("(&rec, self.app.pace.recuts.", "last())"),
                concat!("last_line(&self.app.term, ", "self.app.counts.primary())"),
            ),
            (
                "linux.rs",
                include_str!("linux.rs"),
                concat!("self.pace.recuts.", "resize(&mut self.term, &ws)"),
                concat!("self.probes.", "resized("),
                concat!("(&rec, self.pace.recuts.", "last())"),
                concat!("last_line(&self.term, ", "self.seat.counts().primary())"),
            ),
        ] {
            let (before, after) = crate::app_source::split_once_in(src, call)
                .unwrap_or_else(|| panic!("{name}: the resize"));
            let opened = before.rfind("\n    fn ").expect("its function");
            let body_before = &before[opened..];
            let body_after = after.split_once("\n    fn ").map_or(after, |(b, _)| b);
            assert!(
                body_before.contains(primary),
                "{name}: the primary's last line read before the resize"
            );
            assert!(
                body_after.contains(resized) && body_after.contains(told),
                "{name}: Probes::resized after the resize, with its map"
            );
            assert!(
                !body_before.contains(concat!("term.", "resize(&ws)")),
                "{name}: no resize that tells nothing"
            );
        }
    }

    /// The default forms with single names read: what `[link.path] forms`
    /// listing `"name"` reads.
    const NAMES: Forms = Forms {
        name: Turned::On,
        ..Forms::DEFAULT
    };

    /// A window of `ws` with `bytes` fed after an `OSC 7` naming this
    /// machine's folder `/w`, and its paths with `forms`, the report seen,
    /// every row filed as a local program's.
    fn in_folder<'a>(t: &mut Term<'a>, forms: Forms, bytes: &[u8]) -> Paths {
        feed(t, b"\x1b]7;file://here.example/w\x07");
        let mut p = Paths::new(None, forms);
        p.set(Turned::On);
        p.set_names(crate::link::MachineNames::of(vec![
            "here.example".to_string(),
        ]));
        p.all_written_here();
        let _ = p.observe(t, at());
        feed(t, bytes);
        let _ = p.observe(t, at());
        p
    }

    /// One collection of `t`'s window: what it asks, as paths.
    fn collect(t: &Term<'_>, p: &mut Paths, l: &mut Lines, u: &mut Uris) -> Vec<Candidate> {
        p.begin_collect();
        l.forget();
        let _ = l.scan(
            t,
            at(),
            &mut Links {
                uris: u,
                urls: Turned::On,
                paths: p,
            },
        );
        p.end_collect()
    }

    /// A frame's scan of `t`, outside a collection.
    fn frame(t: &Term<'_>, p: &mut Paths, l: &mut Lines, u: &mut Uris) {
        let _ = l.scan(
            t,
            at(),
            &mut Links {
                uris: u,
                urls: Turned::On,
                paths: p,
            },
        );
    }

    fn paths_of(asked: &[Candidate]) -> Vec<String> {
        asked
            .iter()
            .map(|c| String::from_utf8_lossy(&c.path).into_owned())
            .collect()
    }

    /// **A single name is a candidate when it holds a `.` not at its end,
    /// and a word only once `word` is read**: `README.md`, `.zshrc` and
    /// `a.tar.gz` are names; `README.` (its `.` is prose), `Library`, `1`
    /// and `http://x.y` are not. A name read does not swallow a prefixed
    /// path after an opener inside it. Named break: read every word as a
    /// name (`unprefixed_form` without the `.` test) -- `Library` and `1`
    /// come back.
    #[test]
    fn a_name_holds_a_dot_and_a_word_is_read_only_when_asked() {
        let d = NAMES;
        assert_eq!(
            found("README.md .zshrc a.tar.gz", &d),
            ["README.md", ".zshrc", "a.tar.gz"]
        );
        assert_eq!(
            found("README. Library 1 http://x.y", &d),
            Vec::<String>::new()
        );
        assert_eq!(
            found("(see `Cargo.toml`), and 'notes.txt'.", &d),
            ["Cargo.toml", "notes.txt"]
        );
        assert_eq!(
            found("--file=/tmp/a main.rs:12 a.b:c", &d),
            ["/tmp/a"],
            "a `:` is no name, and the prefixed path inside stays"
        );
        let words = Forms {
            word: Turned::On,
            ..d
        };
        assert_eq!(
            found("README. Library 1 Cargo.toml", &words),
            ["README", "Library", "1", "Cargo.toml"]
        );
        let names = |s: &str, f: &Forms| {
            let u = units(s);
            let mut out = Vec::new();
            tokens(&u, (Edge::Line, Edge::Line), f, &mut out);
            out.iter().map(|t| t.form).collect::<Vec<_>>()
        };
        assert_eq!(
            names(
                "Makefile a.rs src/a.rs",
                &Forms {
                    bare: Turned::On,
                    ..words
                }
            ),
            [Form::Word, Form::Name, Form::Bare]
        );
    }

    /// **The scan's tail test lets every name and word through**: a name's
    /// `.` before any character it may hold, a byte outside ASCII among
    /// them; a word's first character after an opener; and never a `./` or
    /// `../` for a name. Named break: drop the name arm of `starts` -- the
    /// default forms' `README.md` is never read.
    #[test]
    fn the_tail_test_admits_names_and_words() {
        let tail = |s: &[u8]| {
            s.iter()
                .fold(0u64, |a, &b| (a << 8) | u64::from(b))
                .to_be_bytes()
        };
        let d = Forms {
            bare: Turned::Off,
            ..NAMES
        };
        for s in [&b" README.m"[..], b" .z", b"a.\0", b"v1.2"] {
            assert!(starts(tail(s), &d), "{s:?}");
        }
        for s in [&b"a. "[..], b"x./", b"x../", b"Library"] {
            assert!(!starts(tail(s), &d), "{s:?}");
        }
        let words = Forms {
            word: Turned::On,
            ..d
        };
        assert!(starts(tail(b" L"), &words));
        assert!(!starts(tail(b"aL"), &words), "inside a word");
        let none = Forms {
            name: Turned::Off,
            ..d
        };
        assert!(!starts(tail(b" README.m"), &none), "names off");
    }

    /// **A name gets a line only where the worker found it in the
    /// reported folder**: `README.md` there is underlined, `NOTES.md`,
    /// answered missing, is not, and both were asked as files of `/w`.
    /// Named break: draw a path whatever its answer -- `NOTES.md` is lit.
    #[test]
    fn a_name_lights_only_where_the_worker_found_it() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = in_folder(&mut t, NAMES, b"README.md NOTES.md\r\n");
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let asked = collect(&t, &mut p, &mut l, &mut u);
        assert_eq!(paths_of(&asked), ["/w/README.md", "/w/NOTES.md"]);
        let _ = p.merge(asked.into_iter().map(|c| {
            let there = c.path == b"/w/README.md";
            (c.path, if there { Probe::Exists } else { Probe::Missing })
        }));
        l.forget();
        frame(&t, &mut p, &mut l, &mut u);
        assert_eq!(runs(&l), [(0, 0, 8)], "README.md alone");
        assert_eq!(l.struck(), &[], "a missing name is never struck at rest");
    }

    /// **A name is read against the folder it was printed in, and only
    /// there**: printed before the report, none; on the alternate screen,
    /// against the program's folder ([`ProgramDir`]); on the rows `ssh` or a
    /// multiplexer wrote, none, while the row above them is still read.
    /// Named breaks: read a name on any row (drop `Form::Name` from
    /// `relative`) -- the row above the report and the multiplexer's row ask
    /// it.
    #[test]
    fn a_name_is_read_in_its_folder_and_refused_where_baud_cannot_tell() {
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"early.md\r\n");
        let mut p = in_folder(&mut t, NAMES, b"late.md\r\n");
        assert_eq!(
            paths_of(&collect(&t, &mut p, &mut l, &mut u)),
            ["/w/late.md"],
            "the row before the report gets none"
        );
        // The alternate screen: the program's folder.
        feed(&mut t, b"\x1b[?1049h\x1b[Hdoc.md\r\n");
        let _ = p.observe(&t, at());
        assert!(
            collect(&t, &mut p, &mut l, &mut u).is_empty(),
            "no program folder read yet: none"
        );
        let _ = p.set_program_dir(Some(ProgramDir(b"/prog".to_vec())));
        assert_eq!(
            paths_of(&collect(&t, &mut p, &mut l, &mut u)),
            ["/prog/doc.md"]
        );
        let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        for (front, what) in [
            (Front::Multiplexer, "a multiplexer"),
            (Front::Elsewhere, "ssh"),
        ] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut p = in_folder(&mut t, NAMES, b"late.md\r\n");
            p.set_front(front, on_primary(first.below()));
            feed(&mut t, b"pane.md\r\n");
            let _ = p.observe(&t, at());
            assert_eq!(
                paths_of(&collect(&t, &mut p, &mut l, &mut u)),
                ["/w/late.md"],
                "{what}: its row none, the row above it read"
            );
        }
    }

    /// **`names = "hover"` draws no line at rest and lights the name under
    /// the pointer**, and a prefixed path keeps its line. Named break:
    /// ignore the key in `read` -- the name is underlined at rest.
    #[test]
    fn names_hover_lights_only_under_the_pointer() {
        use crate::link::{AtRest, Chord, Hover};
        use baud_core::Focus;
        use baud_domain::{Col, Row};
        let hover = Forms {
            names: NameLook::Hover,
            ..NAMES
        };
        for (forms, want) in [
            (NAMES, vec![(0, 0, 8), (0, 10, 19)]),
            (hover, vec![(0, 10, 19)]),
        ] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut p = in_folder(&mut t, forms, b"README.md ./notes.md\r\n");
            let (mut l, mut u) = (Lines::default(), Uris::default());
            let asked = collect(&t, &mut p, &mut l, &mut u);
            assert_eq!(asked.len(), 2, "both asked, whatever the look");
            let _ = p.merge(asked.into_iter().map(|c| (c.path, Probe::Exists)));
            l.forget();
            frame(&t, &mut p, &mut l, &mut u);
            assert_eq!(runs(&l), want, "{:?}", forms.names);
            let mut h = Hover::default();
            h.set_at_rest(AtRest::Look(baud_render::HoverPaint::Box));
            let _ = h.update_with(
                &t,
                Some((Row::ZERO, Col::at_index(3))),
                at(),
                Chord::Other,
                Focus::Gained,
                &crate::link::MachineNames::NONE,
                &p,
            );
            assert!(
                h.over().is_some(),
                "{:?}: lit under the pointer",
                forms.names
            );
        }
    }

    /// **Under `names = "hover"` a single name found reads no row at rest**,
    /// since it draws nothing there, and is still lit under the pointer; a
    /// path the screen also holds prefixed, in either order, reads rows, and
    /// so does a name under the line look. Named break: count every found
    /// path in `update_reads` -- every frame reads rows for names it never
    /// draws.
    #[test]
    fn names_hover_alone_reads_no_row_at_rest() {
        use crate::link::{AtRest, Chord, Hover};
        use baud_core::Focus;
        use baud_domain::{Col, Row};
        let hover = Forms {
            names: NameLook::Hover,
            ..NAMES
        };
        for (forms, bytes, asked, reads) in [
            (hover, &b"README.md\r\n"[..], "/w/README.md", Reads::No),
            (hover, &b"./notes.md\r\n"[..], "/w/notes.md", Reads::Yes),
            (
                hover,
                &b"README.md ./README.md\r\n"[..],
                "/w/README.md",
                Reads::Yes,
            ),
            (
                hover,
                &b"./README.md README.md\r\n"[..],
                "/w/README.md",
                Reads::Yes,
            ),
            (NAMES, &b"README.md\r\n"[..], "/w/README.md", Reads::Yes),
        ] {
            let what = (forms.names, String::from_utf8_lossy(bytes));
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut p = in_folder(&mut t, forms, bytes);
            let (mut l, mut u) = (Lines::default(), Uris::default());
            let found = collect(&t, &mut p, &mut l, &mut u);
            assert_eq!(paths_of(&found), [asked], "{what:?}: one key a file");
            let _ = p.merge(found.into_iter().map(|c| (c.path, Probe::Exists)));
            assert_eq!(p.reads == Reads::Yes, reads == Reads::Yes, "{what:?}");
            let mut h = Hover::default();
            h.set_at_rest(AtRest::Look(baud_render::HoverPaint::Box));
            let _ = h.update_with(
                &t,
                Some((Row::ZERO, Col::at_index(3))),
                at(),
                Chord::Other,
                Focus::Gained,
                &crate::link::MachineNames::NONE,
                &p,
            );
            assert!(h.over().is_some(), "{what:?}: lit under the pointer");
        }
    }

    /// **A batch asks every prefixed path first and at most [`NAMES_MAX`]
    /// names**: 600 names above 10 `./` paths ask all 10 and 256 names,
    /// and the screen stays owed for the rest, which the next batch asks.
    /// Named break: no sub-cap (names into `queue`) -- 512 names fill the
    /// batch first and no `./` path is asked.
    #[test]
    fn a_batch_asks_paths_first_and_at_most_256_names() {
        let ws = WinSize {
            ws_col: 200,
            ws_row: 40,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let mut text = String::new();
        for n in 0..600 {
            text.push_str(&format!("n{n:03}.x "));
            if n % 25 == 24 {
                text.push_str("\r\n");
            }
        }
        for n in 0..10 {
            text.push_str(&format!("./p{n} "));
        }
        let mut p = in_folder(&mut t, NAMES, text.as_bytes());
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let asked = collect(&t, &mut p, &mut l, &mut u);
        let names = asked.iter().filter(|c| c.form == Form::Name).count();
        let dots = asked.iter().filter(|c| c.form == Form::Dot).count();
        assert_eq!((dots, names), (10, NAMES_MAX));
        assert_eq!(p.owed(), Owed::Yes, "the rest are owed");
        let _ = p.merge(asked.into_iter().map(|c| (c.path, Probe::Missing)));
        let again = collect(&t, &mut p, &mut l, &mut u);
        assert_eq!(again.len(), NAMES_MAX, "the next 256");
        assert!(again.iter().all(|c| c.form == Form::Name));
        assert!(
            p.known.len() <= 600 + 10,
            "known holds the screen's answers, no more: {}",
            p.known.len()
        );
    }

    /// **A frame looks nothing up for a name**: it only owes a batch, the
    /// new name unanswered, and nothing in this file calls the file system.
    /// Named break: a `std::fs::symlink_metadata` call in `read` -- the
    /// source check fails.
    #[test]
    fn no_name_is_looked_up_in_a_frame() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = in_folder(&mut t, NAMES, b"README.md\r\n");
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let asked = collect(&t, &mut p, &mut l, &mut u);
        let _ = p.merge(asked.into_iter().map(|c| (c.path, Probe::Exists)));
        feed(&mut t, b"other.md\r\n");
        frame(&t, &mut p, &mut l, &mut u);
        assert_eq!(p.owed(), Owed::Yes, "a frame owes");
        assert_eq!(p.result(b"/w/other.md"), None, "and asks nothing");
        let src = include_str!("paths.rs");
        let code = &src[..src.find("#[cfg(test)]\nmod tests").expect("the tests")];
        for call in ["std::fs", "symlink_metadata", "read_link", "File::open"] {
            assert!(!code.contains(call), "{call}");
        }
    }

    /// **A scroll reads the names of the rows it brought in, and no other;
    /// a still frame reads none; an answer that a file is gone takes its
    /// line away.** Six rows of names found, one line more: the scan looks
    /// at the new row's and the row above's names only, and every moved
    /// name keeps its line, a row up. Then `f1.md` is answered gone, the
    /// batch moves, and the next scan draws no line on it. Named break:
    /// compare each row with what the same row held (no `Lines::shift`) --
    /// the scroll looks at every row's names.
    #[test]
    fn a_scroll_reads_only_the_new_rows_names() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut at = at();
        let fed = |t: &mut Term<'_>, at: &mut Scrolled, b: &[u8]| {
            let before = t.rotation();
            feed(t, b);
            if let baud_core::Moved::Retired(n) = t.moved_since(before) {
                *at = at.advanced(n);
            }
        };
        fed(&mut t, &mut at, b"\x1b]7;file://here.example/w\x07");
        let mut p = Paths::new(None, NAMES);
        p.set(Turned::On);
        p.set_names(crate::link::MachineNames::of(vec![
            "here.example".to_string(),
        ]));
        p.all_written_here();
        let _ = p.observe(&t, at);
        for n in 0..6 {
            let nl = if n == 0 { "" } else { "\r\n" };
            fed(&mut t, &mut at, format!("{nl}f{n}.md g{n}.rs").as_bytes());
        }
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let mut scan = |p: &mut Paths, l: &mut Lines, t: &Term<'_>, at: Scrolled| {
            let _ = p.observe(t, at);
            l.scan(
                t,
                at,
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: p,
                },
            )
        };
        p.begin_collect();
        let _ = scan(&mut p, &mut l, &t, at);
        let asked = p.end_collect();
        assert_eq!(asked.len(), 12, "the control: every name asked");
        let _ = p.merge(asked.into_iter().map(|c| (c.path, Probe::Exists)));
        l.forget();
        assert_eq!(scan(&mut p, &mut l, &t, at).candidates, 12);
        assert_eq!(runs(&l).len(), 12, "every name lined");
        assert_eq!(
            scan(&mut p, &mut l, &t, at).rows,
            0,
            "a still frame reads nothing"
        );
        fed(&mut t, &mut at, b"\r\nf6.md g6.rs");
        let s = scan(&mut p, &mut l, &t, at);
        assert_eq!(
            s.candidates, 4,
            "the new row's two names and the row above's"
        );
        assert_eq!(
            runs(&l).len(),
            10,
            "the moved names keep their lines; the new ones unanswered"
        );
        assert!(
            runs(&l).contains(&(0, 0, 4)),
            "f1.md, a row up: {:?}",
            runs(&l)
        );
        // f1.md answered gone at the next batch: its line goes.
        assert_eq!(
            p.merge([(b"/w/f1.md".to_vec(), Probe::Missing)]),
            Moved::Yes
        );
        l.forget();
        let _ = scan(&mut p, &mut l, &t, at);
        assert!(!runs(&l).contains(&(0, 0, 4)), "{:?}", runs(&l));
    }
}

/// **A link across a row its writer ended**, read the four ways a link is
/// read -- the line at rest, the look under the pointer, the chord's
/// preview and a click -- through `Paths`, which holds `[link] join_rows`
/// and the rows' provenance (`PathsAt::joins`).
#[cfg(test)]
mod joined_rows {
    use super::*;
    use crate::lined::Lines;
    use crate::link::{
        AtRest, Chord, Clicks, Gesture, Hover, Press, Spot, Under, gesture, under_with,
    };
    use baud_core::{Focus, Owned};
    use baud_domain::{BytePos, Col, Cols, ReadLen, Row, WinSize};

    type Runs = Vec<(Row, Col, Col)>;

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        for c in bytes.chunks(4096) {
            t.write_at(BytePos::ZERO, c);
            t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
        }
    }

    fn at() -> Scrolled {
        Scrolled::START.advanced(Rows::of_count(1_000))
    }

    fn line(y: Row) -> AbsLine {
        at().line(Rows::ZERO, y)
    }

    fn paths() -> Paths {
        let mut p = Paths::new(Some(b"/home/user".to_vec()), Forms::DEFAULT);
        p.set(Turned::On);
        p
    }

    /// The scan as the driver makes it, every path asked answered: there
    /// if it starts `/usr/`.
    fn scan(t: &Term<'_>, p: &mut Paths, l: &mut Lines) -> Runs {
        let mut u = Uris::default();
        let mut once = |p: &mut Paths, l: &mut Lines| {
            let _ = l.scan(
                t,
                at(),
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: p,
                },
            );
        };
        let _ = p.observe(t, at());
        p.begin_collect();
        once(p, l);
        let asked = p.end_collect();
        let answers = asked.into_iter().map(|c| {
            let there = c.path.starts_with(b"/usr/");
            (c.path, if there { Probe::Exists } else { Probe::Missing })
        });
        let _ = p.merge_into(answers, l);
        once(p, l);
        l.lines().iter().map(|s| (s.row, s.from, s.to)).collect()
    }

    fn spans(s: &[baud_render::LinkSpan]) -> Runs {
        s.iter().map(|s| (s.row, s.from, s.to)).collect()
    }

    /// The look under the pointer at `(row, col)`, the chord's preview there,
    /// and what a Cmd-click there opens.
    fn pointed(t: &Term<'_>, p: &Paths, (row, col): (Row, Col)) -> (Runs, Runs, Option<Vec<u8>>) {
        let cell = Some((row, col));
        let mut h = Hover::default();
        h.set_at_rest(AtRest::Look(baud_render::HoverPaint::Box));
        let _ = h.update_with(
            t,
            cell,
            at(),
            Chord::Other,
            Focus::Gained,
            &crate::link::MachineNames::NONE,
            p,
        );
        let over = h.over().map(|o| spans(&o.spans)).unwrap_or_default();
        let mut h = Hover::default();
        let _ = h.update_with(
            t,
            cell,
            at(),
            Chord::Cmd,
            Focus::Gained,
            &crate::link::MachineNames::NONE,
            p,
        );
        let chord = h.look().map(|o| spans(&o.spans)).unwrap_or_default();
        let spot = Spot {
            scrolled: at(),
            back: Rows::ZERO,
            row,
            col,
        };
        let down = under_with(t, spot, Press::Down, Chord::Cmd, Focus::Gained, None, p);
        let opens = match gesture(
            Press::Down,
            Chord::Cmd,
            Clicks::Single,
            Focus::Gained,
            None,
            down.as_ref(),
        ) {
            Gesture::Arm(a) => {
                let up = under_with(t, spot, Press::Up, Chord::Other, Focus::Gained, Some(&a), p);
                match gesture(
                    Press::Up,
                    Chord::Other,
                    Clicks::Single,
                    Focus::Gained,
                    Some(&a),
                    up.as_ref(),
                ) {
                    Gesture::Open(Under::Bare(b)) => b.uri().ok().map(<[u8]>::to_vec),
                    Gesture::Open(Under::Path(q)) => Some(q.uri().as_bytes().to_vec()),
                    Gesture::Open(Under::Osc8(o)) => o.uri(t).map(<[u8]>::to_vec),
                    _ => None,
                }
            }
            _ => None,
        };
        (over, chord, opens)
    }

    fn expand(runs: &[(Row, Col, Col)]) -> Vec<(usize, usize)> {
        let mut v: Vec<_> = runs
            .iter()
            .flat_map(|&(r, a, b)| (a.index()..=b.index()).map(move |c| (r.index(), c)))
            .collect();
        v.sort_unstable();
        v
    }

    const URL: &str = "see https://example.com/a/very/long/path/to/some/resource/index.html ok";

    /// A multiplexer's full-width pane of `w` columns: each row of `text`
    /// written after a cursor move of its own, so the terminal never wraps.
    fn pane(text: &str, w: Cols) -> Vec<u8> {
        let mut out = Vec::new();
        for (y, chunk) in text.as_bytes().chunks(w.index()).enumerate() {
            out.extend_from_slice(format!("\x1b[{};1H", y + 1).as_bytes());
            out.extend_from_slice(chunk);
        }
        out
    }

    /// **A link over two rows a multiplexer repainted reads the same four
    /// ways**: lined on both rows at rest, the look and the chord over both
    /// from a pointer on either row, and a click there opens the whole
    /// address -- a URI, and a printed path whose file is there. A pane
    /// beside a sidebar, narrower than the screen, is not joined: its rows
    /// end short of the last column, or the next starts in a space. The
    /// controls are the soft-wrapped twins. Perturbations: drop the join
    /// from `read_window` -- the look, the chord and the click take one row;
    /// drop it from `read_group` -- the line at rest does.
    #[test]
    fn a_link_across_a_row_the_writer_ended_reads_the_same_four_ways() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let path = "see /usr/share/doc/some-package/examples/a-long-file-name.txt ok";
        let cases: [(&str, Vec<u8>, Runs, &[u8]); 4] = [
            (
                "url, soft",
                URL.as_bytes().to_vec(),
                vec![
                    (Row::from_index(0), Col::at_index(4), Col::at_index(39)),
                    (Row::from_index(1), Col::at_index(0), Col::at_index(27)),
                ],
                b"https://example.com/a/very/long/path/to/some/resource/index.html",
            ),
            (
                "url, pane",
                pane(URL, Cols::of_index(40)),
                vec![
                    (Row::from_index(0), Col::at_index(4), Col::at_index(39)),
                    (Row::from_index(1), Col::at_index(0), Col::at_index(27)),
                ],
                b"https://example.com/a/very/long/path/to/some/resource/index.html",
            ),
            (
                "path, soft",
                path.as_bytes().to_vec(),
                vec![
                    (Row::from_index(0), Col::at_index(4), Col::at_index(39)),
                    (Row::from_index(1), Col::at_index(0), Col::at_index(20)),
                ],
                b"file:///usr/share/doc/some-package/examples/a-long-file-name.txt",
            ),
            (
                "path, pane",
                pane(path, Cols::of_index(40)),
                vec![
                    (Row::from_index(0), Col::at_index(4), Col::at_index(39)),
                    (Row::from_index(1), Col::at_index(0), Col::at_index(20)),
                ],
                b"file:///usr/share/doc/some-package/examples/a-long-file-name.txt",
            ),
        ];
        for (name, bytes, want, uri) in cases {
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            feed(&mut t, &bytes);
            let (mut p, mut l) = (paths(), Lines::default());
            p.all_written_here();
            assert_eq!(scan(&t, &mut p, &mut l), want, "{name}: at rest");
            for (row, col) in [(0, 10), (1, 5)] {
                let (over, chord, opens) =
                    pointed(&t, &p, (Row::from_index(row), Col::at_index(col)));
                assert_eq!(over, want, "{name} ({row}, {col}): the look");
                assert_eq!(chord, want, "{name} ({row}, {col}): the chord");
                assert_eq!(
                    opens.as_deref(),
                    Some(uri),
                    "{name} ({row}, {col}): the click"
                );
            }
        }
        // A pane beside a sidebar: the rows below the first take no line.
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let mut bytes = Vec::new();
        for (y, chunk) in URL.as_bytes().chunks(31).enumerate() {
            bytes.extend_from_slice(
                format!("\x1b[{};1H sidebar \x1b[{};10H", y + 1, y + 1).as_bytes(),
            );
            bytes.extend_from_slice(chunk);
        }
        feed(&mut t, &bytes);
        let (mut p, mut l) = (paths(), Lines::default());
        p.all_written_here();
        let rest = scan(&t, &mut p, &mut l);
        assert!(
            rest.iter().all(|r| r.0 == Row::ZERO),
            "a narrower pane is not joined: {rest:?}"
        );
        assert!(
            pointed(&t, &p, (Row::from_index(1), Col::at_index(12)))
                .0
                .is_empty(),
            "nor read on under the pointer"
        );
    }

    /// **One address a program wrote across two reads stays joined**: its
    /// first row filed by one read and its second by the next, both named
    /// local, read as one provenance, so the address joins and opens whole.
    /// The control: the second read's writer not named -- the rows part.
    /// What joins two rows is each row's origin, a function of the facts
    /// filed over it, not which read filed it.
    #[test]
    fn one_address_across_two_reads_stays_joined() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let whole: &[u8] = b"https://example.com/a/very/long/path/to/some/resource/index.html";
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, &pane(URL, Cols::of_index(40)));
        for (second, joined) in [(Seen::Named(Front::Local), true), (Seen::NotNamed, false)] {
            let (mut p, mut l) = (paths(), Lines::default());
            p.written_under(
                Seen::Named(Front::Local),
                LineSpan::of(line(Row::ZERO), line(Row::ZERO)),
            );
            p.written_under(
                second,
                LineSpan::of(line(Row::from_index(1)), line(Row::from_index(1))),
            );
            let opens = pointed(&t, &p, (Row::from_index(1), Col::at_index(5))).2;
            let _ = scan(&t, &mut p, &mut l);
            match joined {
                true => assert_eq!(opens.as_deref(), Some(whole), "one program's"),
                false => assert_ne!(opens.as_deref(), Some(whole), "the control"),
            }
        }
    }

    /// **Rows of another provenance never join, and `join_rows = "off"`
    /// joins nothing**: a row a remote program wrote above one a local
    /// program wrote stays apart, cut, so no line and a click opens what the
    /// row holds; two local rows join. A program named after the scan moves
    /// the join with no row changed. The config's key reaches `Paths` as the
    /// drivers hand it over. Perturbations: drop the origin compare from
    /// `Paths::joins` -- the remote row joins; drop the scan's asking again
    /// in `Lines::scan` -- the late naming keeps the join; ignore
    /// `set_join` -- off joins.
    #[test]
    fn provenance_and_join_rows_hold_rows_apart() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let whole: &[u8] = b"https://example.com/a/very/long/path/to/some/resource/index.html";
        let cut: &[u8] = b"https://example.com/a/very/long/path";
        let both = vec![
            (Row::from_index(0), Col::at_index(4), Col::at_index(39)),
            (Row::from_index(1), Col::at_index(0), Col::at_index(27)),
        ];
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, &pane(URL, Cols::of_index(40)));
        // Another provenance: apart.
        let (mut p, mut l) = (paths(), Lines::default());
        p.written_under(
            Seen::Named(Front::Elsewhere),
            LineSpan::of(line(Row::ZERO), line(Row::ZERO)),
        );
        p.written_under(
            Seen::Named(Front::Local),
            LineSpan::of(line(Row::from_index(1)), line(Row::from_index(1))),
        );
        assert_eq!(scan(&t, &mut p, &mut l), [], "remote above local: cut");
        assert_eq!(
            pointed(&t, &p, (Row::from_index(0), Col::at_index(10)))
                .2
                .as_deref(),
            Some(cut)
        );
        // One provenance: joined.
        let (mut p, mut l) = (paths(), Lines::default());
        p.written_under(
            Seen::Named(Front::Local),
            LineSpan::of(line(Row::ZERO), line(Row::from_index(1))),
        );
        assert_eq!(scan(&t, &mut p, &mut l), both, "both local");
        assert_eq!(
            pointed(&t, &p, (Row::from_index(1), Col::at_index(5)))
                .2
                .as_deref(),
            Some(whole)
        );
        // Named after the scan: the next scan, with no row changed, parts them.
        let (mut p, mut l) = (paths(), Lines::default());
        assert_eq!(scan(&t, &mut p, &mut l), both, "neither named yet");
        p.written_under(
            Seen::Named(Front::Elsewhere),
            LineSpan::of(line(Row::from_index(1)), line(Row::from_index(1))),
        );
        assert_eq!(scan(&t, &mut p, &mut l), [], "the lower row named remote");
        // `join_rows = "off"`, read by the config and handed to `Paths`.
        let mut c = crate::config::Config::defaults();
        assert!(c.apply("[link]\njoin_rows = \"off\"\n").is_empty());
        let (mut p, mut l) = (paths(), Lines::default());
        assert_eq!(p.set_join(c.link.join_read()), Moved::Yes);
        assert_eq!(scan(&t, &mut p, &mut l), [], "off: cut");
        let (over, chord, opens) = pointed(&t, &p, (Row::from_index(0), Col::at_index(10)));
        assert_eq!(
            (over, chord),
            (
                vec![],
                vec![(Row::from_index(0), Col::at_index(4), Col::at_index(39))]
            ),
            "off: the chord marks the row, the look nothing"
        );
        assert_eq!(
            opens.as_deref(),
            Some(cut),
            "off: a click opens the row's part"
        );
    }

    /// **An `OSC 8` link a multiplexer repainted a row at a time is one
    /// link**: each row placed by its own cursor move and opening the link
    /// again, the same URI and no `id`, as a multiplexer that forwards a
    /// program's links but not their `id` sends it. Every cell names one
    /// slot of the core's link table, keyed by `id` and URI, so the line at
    /// rest, the look and the chord cover every row and a click opens the
    /// URI, with `join_rows` on or off: it is the link table's tie, not the
    /// join. `[[source: osc8-spec]]` ties cells by `id`, and leaves cells
    /// without one to the terminal's heuristics. Perturbation: span only the
    /// pointer's row in `link_spans_into` -- the look and the chord take one
    /// row.
    #[test]
    fn an_osc8_link_reopened_on_each_row_is_one_link() {
        let ws = WinSize {
            ws_col: 20,
            ws_row: 6,
        };
        let text = "a-long-file-name-that-runs-over-three-rows";
        let mut bytes = Vec::new();
        for (y, chunk) in text.as_bytes().chunks(20).enumerate() {
            bytes.extend_from_slice(
                format!("\x1b[{};1H\x1b]8;;https://e.example/f\x1b\\", y + 1).as_bytes(),
            );
            bytes.extend_from_slice(chunk);
            bytes.extend_from_slice(b"\x1b]8;;\x1b\\");
        }
        let want = vec![
            (Row::from_index(0), Col::at_index(0), Col::at_index(19)),
            (Row::from_index(1), Col::at_index(0), Col::at_index(19)),
            (Row::from_index(2), Col::at_index(0), Col::at_index(1)),
        ];
        for join in [Turned::On, Turned::Off] {
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            feed(&mut t, &bytes);
            let wraps: Vec<_> = (0..3)
                .map(|y| t.view(Rows::ZERO).wrapped(Row::from_index(y)))
                .collect();
            assert_eq!(
                wraps,
                [baud_domain::Wrap::Ended; 3],
                "the control: every row the writer's"
            );
            let (mut p, mut l) = (paths(), Lines::default());
            let _ = p.set_join(join);
            let mut u = Uris::default();
            let _ = l.scan(
                &t,
                at(),
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: &mut p,
                },
            );
            let rest: Runs = l.lines().iter().map(|s| (s.row, s.from, s.to)).collect();
            assert_eq!(rest, want, "{join:?}: at rest");
            for (row, col) in [(0, 3), (1, 7), (2, 1)] {
                let (over, chord, opens) =
                    pointed(&t, &p, (Row::from_index(row), Col::at_index(col)));
                assert_eq!(over, want, "{join:?} ({row}, {col}): the look");
                assert_eq!(chord, want, "{join:?} ({row}, {col}): the chord");
                assert_eq!(
                    opens.as_deref(),
                    Some(&b"https://e.example/f"[..]),
                    "{join:?}: the click"
                );
            }
        }
    }

    /// **The look under a still pointer follows a program named after it
    /// wrote**: over two writer-ended rows neither named yet, joined, the
    /// look covers both; the lower row alone named, local, a frame's refresh
    /// with no byte, no move and no answer reads again and the look goes, the
    /// rows of two provenances and the address cut.
    /// The job watch's remote naming (`Paths::written_remote`) moves the
    /// provenance as a naming does. Perturbations: key the read under the
    /// pointer on the answers alone, not `PathsAt::origins` -- the refresh
    /// keeps the joined look; drop the move from `written_remote`.
    #[test]
    fn the_look_follows_a_program_named_late() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, &pane(URL, Cols::of_index(40)));
        let mut p = paths();
        let mut h = Hover::default();
        h.set_at_rest(AtRest::Look(baud_render::HoverPaint::Box));
        let at_cell = Some((Row::from_index(1), Col::at_index(5)));
        let _ = h.update_with(
            &t,
            at_cell,
            at(),
            Chord::Other,
            Focus::Gained,
            &crate::link::MachineNames::NONE,
            &p,
        );
        let over = h.over().map(|o| spans(&o.spans));
        assert_eq!(
            over,
            Some(vec![
                (Row::from_index(0), Col::at_index(4), Col::at_index(39)),
                (Row::from_index(1), Col::at_index(0), Col::at_index(27))
            ]),
            "neither named: joined"
        );
        // Row 1 named local, row 0 still not: their origins part them.
        p.written_under(
            Seen::Named(Front::Local),
            LineSpan::of(line(Row::from_index(1)), line(Row::from_index(1))),
        );
        let moved = h.refresh_with(
            &t,
            at(),
            Chord::Other,
            Focus::Gained,
            &crate::link::MachineNames::NONE,
            &p,
        );
        assert!(moved, "read again");
        assert_eq!(h.over(), None, "parted: the cut address has no look");
        // The job watch's remote naming moves the provenance too.
        let o = PathsAt::origins(&p);
        p.written_remote(LineSpan::of(line(Row::ZERO), line(Row::ZERO)));
        assert_ne!(PathsAt::origins(&p), o, "written_remote moves it");
    }

    /// **A read that files facts over a row and writes none of its cells
    /// moves the row's look at rest**: a `file://` link shown on rows 0 and
    /// 1, both written here, is drawn on both; a remote read of renditions
    /// alone over row 0 leaves every cell as it was, and the next scan,
    /// with nothing forgotten, draws the link on neither row -- its slot is
    /// judged over every row showing it, and a remote row's file is not
    /// this machine's. A read here that moves no fact reads no row. Named
    /// break: discard what `Paths::file` moved in `written_remote` (no
    /// `refiled`) -- both rows keep the line.
    #[test]
    fn a_read_that_moves_only_facts_reads_its_rows_again() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let link = "\x1b]8;;file:///etc/hosts\x1b\\hosts\x1b]8;;\x1b\\";
        feed(&mut t, format!("{link}\r\n{link}\r\n").as_bytes());
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.all_written_here();
        let (mut l, mut u) = (Lines::default(), Uris::default());
        let mut scan = |t: &Term<'_>, p: &mut Paths, l: &mut Lines| {
            let _ = l.scan(
                t,
                at(),
                &mut Links {
                    uris: &mut u,
                    urls: Turned::On,
                    paths: p,
                },
            );
            l.lines()
                .iter()
                .map(|s| (s.row, s.from, s.to))
                .collect::<Runs>()
        };
        assert_eq!(
            scan(&t, &mut p, &mut l),
            [
                (Row::from_index(0), Col::at_index(0), Col::at_index(4)),
                (Row::from_index(1), Col::at_index(0), Col::at_index(4))
            ],
            "the control: written here, drawn on both rows"
        );
        // Filed local again: no fact moves, and no row is read.
        p.written_by_shell(LineSpan::of(line(Row::ZERO), line(Row::ZERO)));
        assert_eq!(p.refiled, None, "a read that moves no fact reads no row");
        feed(&mut t, b"\x1b[1m\x1b[0m");
        p.written_remote(LineSpan::of(line(Row::ZERO), line(Row::ZERO)));
        assert_eq!(
            p.origin(line(Row::ZERO)),
            Origin::Remote,
            "the control: the read filed row 0 remote"
        );
        assert_eq!(
            p.origin(line(Row::from_index(1))),
            Origin::Local,
            "and row 1 not"
        );
        assert_eq!(
            scan(&t, &mut p, &mut l),
            [],
            "a remote row's file is not this machine's, on either row"
        );
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Writer {
        Soft,
        CupPerRow,
        CrLf,
    }

    /// The printable bytes of `bytes` cut every `w` columns, each row after
    /// the first begun by `writer`; escapes (`ESC ] ... ESC \`) copied whole.
    fn recut(bytes: &[u8], w: Cols, writer: Writer) -> Vec<u8> {
        let (w, mut out) = (w.index(), Vec::new());
        let (mut col, mut row, mut i) = (0, 0, 0);
        while i < bytes.len() {
            if bytes[i] == 0x1b {
                let end = bytes[i..]
                    .windows(2)
                    .position(|p| p == b"\x1b\\")
                    .map_or(bytes.len(), |e| i + e + 2);
                out.extend_from_slice(&bytes[i..end]);
                i = end;
                continue;
            }
            if col == w {
                (row, col) = (row + 1, 0);
                match writer {
                    Writer::CupPerRow => {
                        out.extend_from_slice(format!("\x1b[{};1H", row + 1).as_bytes())
                    }
                    Writer::CrLf => out.extend_from_slice(b"\r\n"),
                    Writer::Soft => {}
                }
            }
            out.push(bytes[i]);
            col += 1;
            i += 1;
        }
        out
    }

    /// **A generator over kind, width, length, offset and how the writer
    /// ends its rows**: a URI, an `OSC 8` link or a printed path, cut at the
    /// width by a soft wrap, a cursor move or CR LF, is lined on every row it
    /// covers, at rest, in the look and the chord from a pointer on its
    /// second cell and its last, and a click there opens all of it. With
    /// `join_rows = "off"`, a link the writer cut is never lined whole: the
    /// soft wraps and `OSC 8` are the control. Perturbations: hold every row
    /// the writer ended apart -- the cut writers fail; ignore `join_rows` in
    /// `Paths::joins` -- the "off" half fails.
    #[test]
    fn a_generator_over_widths_and_row_ends() {
        let mut checked = 0;
        for kind in 0..3 {
            for writer in [Writer::Soft, Writer::CupPerRow, Writer::CrLf] {
                for w in [12usize, 20, 33, 40, 80] {
                    for len in [w + 1, 2 * w - 3, 2 * w + 5, 3 * w + 1] {
                        for x0 in [0, 2, w / 3] {
                            let letters: String =
                                (0..len).map(|i| char::from(b"abcdefgh/"[i % 9])).collect();
                            let (link, uri) = match kind {
                                0 => {
                                    let s = format!(
                                        "https://e.example/{}",
                                        &letters[..len.saturating_sub(18).max(1)]
                                    );
                                    (s.clone(), Some(s))
                                }
                                1 => {
                                    let s = format!(
                                        "/usr/{}",
                                        &letters[..len.saturating_sub(5).max(1)]
                                    );
                                    let s = s.trim_end_matches('/').to_string();
                                    let u = format!("file://{s}");
                                    (s, Some(u))
                                }
                                _ => {
                                    let s = letters.replace('/', "x");
                                    (
                                        format!(
                                            "\x1b]8;;https://e.example/t\x1b\\{s}\x1b]8;;\x1b\\"
                                        ),
                                        None,
                                    )
                                }
                            };
                            let n = match kind {
                                2 => len,
                                _ => link.len(),
                            };
                            if x0 + n > 6 * w || (x0 + n - 1) / w == 0 {
                                continue;
                            }
                            let mut bytes = vec![b' '; x0];
                            bytes.extend_from_slice(link.as_bytes());
                            bytes.extend_from_slice(b" ok");
                            let bytes = match writer {
                                Writer::Soft => bytes,
                                _ => recut(&bytes, Cols::of_index(w), writer),
                            };
                            let ws = WinSize {
                                ws_col: u16::try_from(w).expect("a width"),
                                ws_row: 8,
                            };
                            let mut owned = Owned::new(&ws);
                            let mut t = Term::new(owned.storage(), &ws);
                            feed(&mut t, &bytes);
                            let want: Vec<_> = (x0..x0 + n).map(|i| (i / w, i % w)).collect();
                            let (mut p, mut l) = (paths(), Lines::default());
                            p.all_written_here();
                            let case = format!("kind {kind} {writer:?} w{w} n{n} x{x0}");
                            assert_eq!(expand(&scan(&t, &mut p, &mut l)), want, "{case}: at rest");
                            for &(r, c) in [want[1], want[n - 1]].iter() {
                                let (over, chord, opens) =
                                    pointed(&t, &p, (Row::from_index(r), Col::at_index(c)));
                                assert_eq!(expand(&over), want, "{case} ({r}, {c}): the look");
                                assert_eq!(expand(&chord), want, "{case} ({r}, {c}): the chord");
                                if let Some(u) = &uri {
                                    assert_eq!(
                                        opens.as_deref(),
                                        Some(u.as_bytes()),
                                        "{case} ({r}, {c}): the click"
                                    );
                                }
                            }
                            // Off: a cut link is never lined whole.
                            let (mut p, mut l) = (paths(), Lines::default());
                            p.all_written_here();
                            let _ = p.set_join(Turned::Off);
                            let rest = expand(&scan(&t, &mut p, &mut l));
                            match (writer, kind) {
                                (Writer::Soft, _) | (_, 2) => assert_eq!(rest, want, "{case}: off"),
                                _ => assert!(
                                    rest.iter().all(|&(r, _)| r == 0) && rest != want,
                                    "{case}: off {rest:?}"
                                ),
                            }
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 300, "{checked} cases");
    }
}
