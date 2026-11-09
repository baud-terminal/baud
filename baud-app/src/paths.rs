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
//! **The forms.** A printed `/`, `~/` or `./` starts a path, after the start
//! of a line, a space or one of `([{<"'` and `` ` `` or `=`, and never after
//! a `:`, so neither `https://a/b`'s `//a/b` nor `host:/x`'s `/x` is one.
//! `../`, a bare relative path such as `src/app.rs`, a `:line:col` suffix and
//! a quoted path with spaces are each a [`Forms`] switch, off, for the
//! settings that will name them; none is read until one is turned on. A path
//! runs to the next space, quote, `<`, `>` or `|`; trailing `.,;:!?*` and a
//! closing bracket it did not open are prose, as a URI's are (`link.rs`).
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
use std::collections::HashMap;

use baud_core::{FileHost, Screen, Term};
use baud_domain::{Cols, Rows};
use baud_render::{AbsLine, Scrolled, row_of};

use crate::lined::{At, Mark, Reader, Run, Text, Uris};
use crate::link::{Edge, Host, Ix, MAX, Scheme, Target, Unit, admit_with};
use crate::rules::{NO_RULES, Rules};

/// What the bar calls this machine.
#[cfg(target_os = "macos")]
pub const MACHINE: &str = "this Mac";
#[cfg(not(target_os = "macos"))]
pub const MACHINE: &str = "this computer";

/// The most paths one batch asks about. A screen with more asks about the
/// rest in the batches after it, at most one a second (`probe::Pace`).
pub const BATCH_MAX: usize = 512;

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
    /// `../`: the reported folder's parent. Off by default.
    Parent,
    /// `src/app.rs`, no prefix and a `/` inside: the reported folder. Off by
    /// default.
    Bare,
}

impl Form {
    /// Whether it is read against the reported folder, which a multiplexer
    /// hides.
    pub fn relative(self) -> bool {
        matches!(self, Form::Dot | Form::Parent | Form::Bare)
    }
}

/// **Which forms are read**: the three printed prefixes by default, and the
/// rest only once `[link.path]` names them (`rules.rs`). Nothing else changes when one is
/// turned on: the worker, the mount check and the click are the same for
/// every form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Forms {
    pub absolute: Turned,
    pub home: Turned,
    pub dot: Turned,
    pub parent: Turned,
    pub bare: Turned,
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
        parent: Turned::Off,
        bare: Turned::Off,
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
        }
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

/// **The file a `file://` URI names on this machine**, percent-decoded,
/// and whether its host named this machine: none for another host, a query
/// or a fragment, a `%` without two hex digits, bytes that are not UTF-8,
/// or a control or bidirectional control in the name -- none of which is
/// looked up, so none is struck.
pub fn file_path(uri: &[u8], names: &[String]) -> Option<(Named, Vec<u8>)> {
    let scheme = uri.get(..7)?;
    if !scheme.eq_ignore_ascii_case(b"file://") {
        return None;
    }
    let rest = &uri[7..];
    let slash = rest.iter().position(|&b| b == b'/')?;
    let (host, path) = rest.split_at(slash);
    let named = if host.is_empty() || host.eq_ignore_ascii_case(b"localhost") {
        Named::NoMachine
    } else if names
        .iter()
        .any(|n| !n.is_empty() && host.eq_ignore_ascii_case(n.as_bytes()))
    {
        Named::ThisMachine
    } else {
        return None;
    };
    if path.iter().any(|&b| b == b'?' || b == b'#') {
        return None;
    }
    let hex = |b: u8| {
        char::from(b)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
    };
    let mut out = Vec::with_capacity(path.len());
    let mut i = 0;
    while i < path.len() {
        match path[i] {
            b'%' => {
                let hi = hex(*path.get(i + 1)?)?;
                let lo = hex(*path.get(i + 2)?)?;
                out.push(hi << 4 | lo);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
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
/// or `..`; any `/` once bare paths are read.
pub fn starts(tail: [u8; 8], forms: &Forms) -> bool {
    // The newest byte last: `at(1)` is the one before the `/`.
    let at = |k: usize| tail[7 - k];
    let opens = |k: usize| OPENS.contains(&at(k));
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
        Form::Bare
    } else {
        return None;
    };
    (forms.of(form) == Turned::On).then_some(form)
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
/// characters, one with a character it must not hold, or a bare one with no
/// `/` inside or a `:` (`host:path` names another machine) is none. A bare
/// one starts at a name's character ([`names`]) and ends at a closing
/// bracket it did not open ([`unopened`]), so a Markdown link's path is read
/// without the brackets around it.
pub fn tokens(units: &[Unit], (head, tail): (Edge, Edge), forms: &Forms, out: &mut Vec<Token>) {
    out.clear();
    let n = units.len();
    let mut i = 0;
    while i < n {
        let before = i.checked_sub(1).map(|k| units[k]);
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
        // A relative path with no prefix ends at a closing bracket it did
        // not open: `[t](docs/a.md)`'s `]`, whose wrapper's path the scan
        // reads from there.
        while end < n {
            match part(units[end]) {
                Part::In if form == Form::Bare && unopened(&units[i..end], units[end]) => break,
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
            i = next;
            continue;
        }
        let end = i + trimmed(&units[i..end]).len();
        let path_end = match forms.line_col {
            Turned::On => i + without_line_col(&units[i..end]).len(),
            Turned::Off => end,
        };
        let text = &units[i..path_end];
        let bare_ok = form != Form::Bare
            || (text.iter().any(|u| is(Some(u), '/')) && !text.iter().any(|u| is(Some(u), ':')));
        let prefix = match form {
            Form::Absolute | Form::Bare => 1,
            Form::Home | Form::Dot => 2,
            Form::Parent => 3,
        };
        if bare_ok && path_end - i > prefix {
            out.push(Token {
                span: i..end,
                path: i..path_end,
                form,
            });
        }
        i = next;
    }
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
        Form::Absolute | Form::Home | Form::Parent | Form::Bare => text,
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
/// by the collection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Reads {
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

/// **The lines one program wrote, first to last**, on the terminal's
/// absolute line numbers, so history scrolling and trimming leave them
/// where they were: what a read was, from the line the cursor was on as it
/// began to the line it ended on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineSpan {
    first: AbsLine,
    last: AbsLine,
}

impl LineSpan {
    /// From `first` to `last`, whichever way round they come.
    pub fn of(first: AbsLine, last: AbsLine) -> LineSpan {
        LineSpan {
            first: first.min(last),
            last: first.max(last),
        }
    }

    /// The span and `other` as one, the lines between included.
    pub fn joined(self, other: LineSpan) -> LineSpan {
        LineSpan {
            first: self.first.min(other.first),
            last: self.last.max(other.last),
        }
    }

    /// The last line.
    pub fn last(self) -> AbsLine {
        self.last
    }

    fn holds(self, line: AbsLine) -> bool {
        self.first <= line && line <= self.last
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
    names: Vec<String>,
    dir: Dir,
    /// The folder of the program in front, while the alternate screen is up.
    program_dir: Option<ProgramDir>,
    geometry: Option<(Cols, Rows)>,
    known: HashMap<Vec<u8>, State>,
    /// `OSC 8` `file://` links' answers, kept apart from printed paths' so
    /// one found does not make a frame read rows for printed paths.
    marked: HashMap<Vec<u8>, State>,
    /// `[link] missing`: whether anything is struck.
    missing: Turned,
    /// What the worker last found for each program it was asked about.
    programs: Vec<(Program, Handler)>,
    generation: Generation,
    owed: Owed,
    collect: Option<Collect>,
    reads: Reads,
    /// What is in front, and the last line written while what is in front
    /// now, or anything not known to be local, was.
    front: Front,
    elsewhere_through: Option<AbsLine>,
    multiplexed_through: Option<AbsLine>,
    /// The lines each named program wrote, in order, adjacent ones of one
    /// origin merged, each local or remote ([`Paths::origin`]).
    spans: Vec<(Origin, LineSpan)>,
    key: Vec<u8>,
    tokens: Vec<Token>,
    /// Your `[link.*]` rules, the forms among them, which a read of the link
    /// under the pointer goes by ([`PathsAt::rules`]).
    rules: Rules,
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
    }

    /// Whether anything is read, asked or drawn.
    pub fn marks(&self) -> Marks {
        self.marks
    }

    /// **The worker stopped answering**: every mark goes, and none comes
    /// again in this window.
    pub fn close(&mut self) {
        self.drop_all();
        self.marks = Marks::Closed;
        self.update_reads();
    }

    fn drop_all(&mut self) {
        self.known.clear();
        self.marked.clear();
        // What the worker found for a program goes too: with no worker to
        // ask it again, a kind struck for having no app would stay struck.
        self.programs.clear();
        self.collect = None;
        self.owed = Owed::No;
        self.generation = self.generation.next();
        self.update_reads();
    }

    /// [`Reads`] again, after the marks, the collection or the answers
    /// changed: once a batch, never a frame.
    fn update_reads(&mut self) {
        let draws = self
            .known
            .values()
            .any(|s| s.probe() == Some(Probe::Exists));
        self.reads = match self.marks == Marks::On && (self.collect.is_some() || draws) {
            true => Reads::Yes,
            false => Reads::No,
        };
    }

    /// **A row of the window changed**: a batch is owed, which the next one
    /// due answers by reading every row then. One store a scan.
    fn changed(&mut self) {
        if self.marks == Marks::On {
            self.owed = Owed::Yes;
        }
    }

    /// This machine's names, once read: what an `OSC 7` report must name.
    pub fn set_names(&mut self, names: Vec<String>) {
        self.names = names;
        self.dir.local = self.local_of_kept();
        self.generation = self.generation.next();
    }

    fn local_of_kept(&self) -> Option<Vec<u8>> {
        let (Some(h), path) = self.dir.kept.as_ref()? else {
            return None;
        };
        let named = self
            .names
            .iter()
            .any(|n| !n.is_empty() && h.eq_ignore_ascii_case(n.as_bytes()));
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
        self.owed
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
            self.owed = Owed::Yes;
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
            self.owed = Owed::Yes;
        }
    }

    /// **The reported folder, before a scan**: a report that differs from the
    /// one kept takes effect at the cursor's line, as does a change of size,
    /// whose reflow moves lines; a screen cleared under the cursor takes the
    /// line down to it. Whether the rows a `./` may be read on moved, which
    /// reads every row again.
    pub fn observe(&mut self, term: &Term<'_>, scrolled: Scrolled) -> Moved {
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
        if !same {
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

    /// **What the worker named in front**, at the line the cursor was on
    /// when it was asked: rows to there were written while it, or anything
    /// before it not known to be local, was in front.
    pub fn set_front(&mut self, front: Front, through: AbsLine) {
        match front {
            Front::Elsewhere => self.elsewhere_through = self.elsewhere_through.max(Some(through)),
            Front::Multiplexer => {
                self.multiplexed_through = self.multiplexed_through.max(Some(through));
            }
            Front::Local => {}
        }
        if self.front == Front::Elsewhere && front != Front::Elsewhere {
            self.elsewhere_through = self.elsewhere_through.max(Some(through));
        }
        if self.front == Front::Multiplexer && front != Front::Multiplexer {
            self.multiplexed_through = self.multiplexed_through.max(Some(through));
        }
        if self.front != front {
            self.front = front;
            self.generation = self.generation.next();
        }
    }

    /// **Rows `span` were written while `front` was in front**, named by
    /// the worker after the program had gone: they keep no mark if it is
    /// not local, and no `./` under a multiplexer; and an `OSC 8` link on
    /// them takes its origin from it ([`Paths::origin`]). What is in front
    /// now is [`Paths::set_front`]'s, and does not change.
    pub fn written_under(&mut self, front: Front, span: LineSpan) {
        let through = span.last;
        let (before, mux) = (self.elsewhere_through, self.multiplexed_through);
        match front {
            Front::Elsewhere => self.elsewhere_through = before.max(Some(through)),
            Front::Multiplexer => self.multiplexed_through = mux.max(Some(through)),
            Front::Local => {}
        }
        if (before, mux) != (self.elsewhere_through, self.multiplexed_through) {
            self.generation = self.generation.next();
        }
        // A multiplexer's panes are this machine's programs: local.
        let origin = match front {
            Front::Elsewhere => Origin::Remote,
            Front::Local | Front::Multiplexer => Origin::Local,
        };
        match self.spans.last_mut() {
            Some((o, s))
                if *o == origin && span.first <= s.last.below() && s.first <= span.last =>
            {
                *s = s.joined(span);
            }
            _ => self.spans.push((origin, span)),
        }
    }

    /// **A program not on this machine may have written `span`**, as the
    /// job watch says of a read (`probe::Watched`): an `OSC 8` link on it
    /// takes a remote origin ([`Paths::origin`]), and nothing else changes.
    /// The printed paths' watermark is not raised: the watch names any job
    /// the shell's started, `git pull`'s ssh among them, and the paths above
    /// what it wrote keep their marks.
    pub fn written_remote(&mut self, span: LineSpan) {
        match self.spans.last_mut() {
            Some((Origin::Remote, s)) if span.first <= s.last.below() && s.first <= span.last => {
                *s = s.joined(span);
            }
            _ => self.spans.push((Origin::Remote, span)),
        }
    }

    /// **Where the program that printed `line` runs**: remote if any program
    /// not known to be local wrote to it, local if only local ones did,
    /// unknown if none was named for it.
    pub fn origin(&self, line: AbsLine) -> Origin {
        let mut found = Origin::Unknown;
        for &(origin, s) in &self.spans {
            if s.holds(line) {
                if origin == Origin::Remote {
                    return Origin::Remote;
                }
                found = origin;
            }
        }
        found
    }

    /// **Where the program that printed `link` at `line` ran, for its
    /// click** ([`crate::link::admit_printed`]): its line's
    /// ([`Paths::origin`]), for a link marked with `OSC 8` and for one read
    /// from the text alike, since a `file://` URI a remote program prints
    /// as plain text names its machine's file just as a marked one does. A
    /// printed path is asked about by its own check, and has none.
    pub fn printed_origin(&self, link: &crate::link::Under, line: AbsLine) -> Option<Origin> {
        use crate::link::Under;
        match link {
            Under::Osc8(_) | Under::Bare(_) => Some(self.origin(line)),
            Under::Path(_) => None,
        }
    }

    /// **The spans that have left history dropped**: those ending above
    /// `oldest`, the oldest line kept, so the set is bounded by history.
    pub fn trim_spans(&mut self, oldest: AbsLine) {
        self.spans.retain(|(_, s)| s.last >= oldest);
    }

    /// **Another program is in front than the worker last named**: until it
    /// is named, rows to `through` keep what the last one made of them, and
    /// rows after it are read as the next answer says.
    pub fn front_moved(&mut self, through: AbsLine) {
        if self.front == Front::Elsewhere {
            self.elsewhere_through = self.elsewhere_through.max(Some(through));
            self.front = Front::Local;
            self.generation = self.generation.next();
        }
    }

    /// **Nothing is owed**: the program in front was asked about and is the
    /// same, not known to be local.
    pub fn settled(&mut self) {
        self.owed = Owed::No;
    }

    /// **A collection starts**: the scan that follows, of every row of the
    /// window, asks about what it finds unanswered; the answers it finds stay
    /// and the rest go.
    pub fn begin_collect(&mut self) {
        self.collect = Some(Collect::default());
        self.update_reads();
    }

    /// **The collection's batch**: what the screen holds that has no answer,
    /// at most [`BATCH_MAX`], each once. The screen's answers replace the
    /// kept ones, so what is off the screen is forgotten; a screen with more
    /// than a batch stays owed.
    pub fn end_collect(&mut self) -> Vec<Candidate> {
        let Some(c) = self.collect.take() else {
            return Vec::new();
        };
        self.known = c.kept;
        self.marked = c.marked;
        self.owed = match c.over {
            Over::Yes => Owed::Yes,
            Over::No => Owed::No,
        };
        self.update_reads();
        c.queue
    }

    /// **A batch's answers**: each kept where it was asked, and the
    /// generation moved when one changed, so the lines and the link under the
    /// pointer are read again once a batch. An answer to a path the screen no
    /// longer holds is dropped.
    pub fn merge(&mut self, probes: impl IntoIterator<Item = (Vec<u8>, Probe)>) -> Moved {
        let mut moved = Moved::No;
        for (path, probe) in probes {
            for map in [&mut self.known, &mut self.marked] {
                if let Some(s) = map.get_mut(&path) {
                    // An answer asked again that came back the same moves
                    // nothing drawn.
                    if s.probe() != Some(probe) {
                        moved = Moved::Yes;
                    }
                    *s = State::Is(probe);
                }
            }
        }
        if moved == Moved::Yes {
            self.generation = self.generation.next();
            self.update_reads();
        }
        moved
    }

    /// **The rows a path on a logical line from `first` to `last` may be
    /// read on**, on `screen`: whether any form is, and whether the relative
    /// ones are, against the folder they would be read against.
    fn readable(&self, text: &Text<'_>, form: Form) -> bool {
        self.readable_on(text.lines, text.screen, form)
    }

    /// [`Paths::readable`], of the lines `first` to `last` on `screen`.
    fn readable_on(&self, (first, last): (AbsLine, AbsLine), screen: Screen, form: Form) -> bool {
        if self.marks != Marks::On
            || self.front == Front::Elsewhere
            || self.elsewhere_through.is_some_and(|t| last <= t)
        {
            return false;
        }
        if !form.relative() {
            return true;
        }
        if self.front == Front::Multiplexer || self.multiplexed_through.is_some_and(|t| first <= t)
        {
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
            Form::Dot | Form::Parent | Form::Bare => {
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
                    self.owed = Owed::Yes;
                }
                continue;
            }
            if !self.resolve(text.units, t, text.screen) {
                continue;
            }
            looked += 1;
            let state = self.known.get(self.key.as_slice()).copied();
            if let Some(c) = &mut self.collect {
                if c.kept.get(self.key.as_slice()) == Some(&State::Asked) {
                    // Printed again lower down: asked at its last line.
                    if let Some(q) = c.queue.iter_mut().find(|q| q.path == self.key) {
                        q.line = q.line.max(text.lines.1);
                    }
                } else if !c.kept.contains_key(self.key.as_slice()) {
                    match state {
                        Some(State::Is(p)) => {
                            c.kept.insert(self.key.clone(), State::Is(p));
                        }
                        s if c.queue.len() < BATCH_MAX => {
                            let s = match s {
                                Some(State::Stale(p)) => State::Stale(p),
                                _ => State::Asked,
                            };
                            c.kept.insert(self.key.clone(), s);
                            c.queue.push(Candidate {
                                path: self.key.clone(),
                                form: t.form,
                                line: text.lines.1,
                            });
                        }
                        Some(s @ State::Stale(_)) => {
                            c.kept.insert(self.key.clone(), s);
                            c.over = Over::Yes;
                        }
                        Some(State::Asked) | None => c.over = Over::Yes,
                    }
                }
            }
            match state.and_then(State::probe) {
                Some(Probe::Exists) if opens_as_uri(&self.key) => {
                    found.push(Run {
                        units: t.span.clone(),
                        mark: Mark::Line,
                    });
                }
                Some(_) => {}
                None if self.collect.is_none() && state.is_none() => self.owed = Owed::Yes,
                None => {}
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
    fn file_link(&mut self, uri: &[u8], at: At) -> Mark {
        if self.marks != Marks::On || self.missing != Turned::On {
            return Mark::Line;
        }
        let Some((named, path)) = file_path(uri, &self.names) else {
            return Mark::Line;
        };
        if !self.readable_on((at.line, at.line), at.screen, Form::Absolute) {
            if self.front == Front::Elsewhere {
                self.owed = Owed::Yes;
            }
            return Mark::Line;
        }
        let state = self.marked.get(path.as_slice()).copied();
        if let Some(c) = &mut self.collect {
            if c.marked.get(path.as_slice()) == Some(&State::Asked) {
                // Linked again lower down: asked at its last line.
                if let Some(q) = c.queue.iter_mut().find(|q| q.path == path) {
                    q.line = q.line.max(at.line);
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
                            line: at.line,
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
            self.owed = Owed::Yes;
        }
        match (named, state.and_then(State::probe)) {
            (Named::ThisMachine, Some(Probe::Missing)) => Mark::Struck,
            _ => Mark::Line,
        }
    }

    /// Whether a line ending in `tail`, its last eight ASCII bytes, the
    /// newest last, may hold a path worth reading.
    fn wants(&self, tail: [u8; 8]) -> bool {
        self.reads == Reads::Yes && starts(tail, &self.forms)
    }
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
pub fn on_found(asked: &Asked, path: &[u8], host: crate::link::Host<'_>, rules: &Rules) -> Found {
    let Asked::Printed(lc) = asked else {
        return Found::Reveal;
    };
    if *rules.path_act() == crate::rules::PathAct::Copy {
        return Found::Copy;
    }
    let uri = file_uri(path);
    if crate::link::admit(uri.as_bytes(), host).is_err() {
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

    fn rules(&self) -> &Rules {
        &self.rules
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
            Form::Dot | Form::Parent | Form::Bare => {
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
                let (_, path) = file_path(target.as_str().as_bytes(), &self.names)?;
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

    fn read(&mut self, text: Text<'_>, found: &mut Vec<Run>) -> Looked {
        let mut n = 0;
        if self.urls == Turned::On {
            let paths = &*self.paths;
            n += self
                .uris
                .read_marking(text, found, &|t| paths.target_mark(t));
        }
        if self.paths.reads == Reads::Yes {
            n += self.paths.read(text, found).0;
            found.sort_unstable_by_key(|r| r.units.start);
        }
        Looked(n)
    }

    fn marked(&mut self, uri: &[u8], at: At) -> Option<Mark> {
        if self.urls != Turned::On {
            return None;
        }
        let t = admit_with(uri, Host(self.uris.host()), self.uris.rules()).ok()?;
        Some(match t.scheme() {
            Scheme::File => self.paths.file_link(uri, at),
            _ => self.paths.target_mark(&t),
        })
    }

    fn changed(&mut self) {
        self.paths.changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lined::Lines;
    use crate::link::{Host, admit};
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

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
        if p.merge(answers) == Moved::Yes {
            l.forget();
        }
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

    /// **The default forms are exactly `/`, `~/` and `./`**, after an
    /// opener, and never inside a URI or after `host:`; trailing prose and
    /// an unopened bracket are cut, and a lone prefix is no path. The other
    /// forms are off until turned on, and each reads once it is.
    /// Perturbation: read `:` as an opener; default `parent` on; drop the
    /// length check after the prefix.
    #[test]
    fn the_default_forms_are_the_three_prefixes() {
        let d = Forms::DEFAULT;
        assert_eq!(
            found("see /etc/hosts, and ~/notes.txt.", &d),
            ["/etc/hosts", "~/notes.txt"]
        );
        assert_eq!(
            found("run ./build.sh (or /usr/bin/x)", &d),
            ["./build.sh", "/usr/bin/x"]
        );
        assert_eq!(
            found("https://a.example/b host:/x a/b /", &d),
            Vec::<String>::new()
        );
        assert_eq!(found("../up src/app.rs /x/y.rs:12:3", &d), ["/x/y.rs:12:3"]);
        assert_eq!(found("//server/share ~/ ./", &d), Vec::<String>::new());
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
        assert_eq!(found("open \"/tmp/a b/c now", &spaces), ["/tmp/a"]);
        assert_eq!(
            found("open \"/tmp/a b/c\" now", &d),
            ["/tmp/a"],
            "off: a space ends it"
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
    /// start the text reads, its tail at the `/` says so. Perturbation: drop
    /// the `~` arm of `starts`.
    #[test]
    fn the_tail_test_admits_every_start() {
        let tail = |s: &str| {
            s.bytes()
                .fold(0u64, |a, b| (a << 8) | u64::from(b))
                .to_be_bytes()
        };
        let d = Forms::DEFAULT;
        for s in [" /", "(/", "=/", " ~/", " ./", "/"] {
            assert!(starts(tail(s), &d), "{s:?}");
        }
        for s in ["a/", ":/", "//", " ../"] {
            assert!(!starts(tail(s), &d), "{s:?}");
        }
        let parent = Forms {
            parent: Turned::On,
            ..d
        };
        assert!(starts(tail(" ../"), &parent));
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
        let t = admit(uri.as_bytes(), Host("")).expect("admitted");
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
        p.set_names(vec!["here.example".to_string()]);
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

    /// **Rows written while a program not known to be local was in front get
    /// no mark after it ends, and a multiplexer takes only `./`.**
    /// Perturbation: clear `elsewhere_through` when the front comes back
    /// local; read `./` under a multiplexer.
    #[test]
    fn rows_from_ssh_keep_no_mark_after_it_ends() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b]7;file://here.example/w\x07");
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.set_names(vec!["here.example".to_string()]);
        let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
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
        p.set_front(Front::Multiplexer, first.below());
        assert_eq!(
            ask(&mut p),
            [b"/etc/hosts".to_vec()],
            "a multiplexer: no ./"
        );
        p.set_front(Front::Elsewhere, first.below());
        assert!(ask(&mut p).is_empty(), "ssh in front: nothing");
        p.set_front(Front::Local, first.below());
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
        let bar = preview_text(&found, &t, Host(""), Cols::of_index(58)).expect("a bar");
        assert!(bar.starts_with(&format!("On {MACHINE}")), "{bar}");
        assert!(bar.ends_with("file:///etc/hosts"), "{bar}");
        let gone = read(15).expect("a refused link");
        let bar = preview_text(&gone, &t, Host(""), Cols::of_index(58)).expect("a bar");
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
                Host(""),
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
        p.set_names(vec![HERE.to_string()]);
        set(&mut p);
        let mut u = Uris::default();
        u.set_host(HERE.to_string());
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
        if p.merge(answers) == Moved::Yes {
            l.forget();
        }
        links(&mut p, &mut l);
        (runs(&l), struck(&l), asked, p)
    }

    /// **An `OSC 8` `file://` link naming this machine, its file gone, is
    /// struck at rest and not underlined**; one found is underlined; and with
    /// `missing = "off"` the gone one is underlined as before and nothing is
    /// asked for it. Perturbations: strike on `Named::NoMachine` in
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
                Probe::Exists => match on_found(&Asked::Link, &path, Host(HERE), rules) {
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
        let path = "[link.path]\ncommand = [\"/bin/echo\", \"{path}\"]\n";
        let rule = |r: &str| {
            let mut c = crate::config::Config::defaults();
            let said = c.apply(&format!("{path}{r}"));
            assert!(said.is_empty(), "{r}: {said:?}");
            c.link.rules
        };
        let copy = rule("[link.file]\naction = \"copy\"\n");
        let command = rule("[link.file]\ncommand = [\"/bin/echo\", \"{url}\"]\n");
        let none = rule("");
        let mut came = Vec::new();
        for (name, rules) in [("copy", &copy), ("command", &command)] {
            let t = crate::link::admit_with(uri, Host(HERE), rules).expect("the rule's");
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
        let plain = crate::link::admit_with(uri, Host(HERE), &none).expect("admitted");
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
        let opens = admit(b"https://a.example/", Host("")).expect("admitted");
        assert_eq!(opens.action(), crate::link::Action::Open);
        assert_eq!(
            click(&opens, &AllGone),
            Click::Open,
            "an open is never asked again"
        );
        let reveals = admit(b"file:///gone", Host("")).expect("admitted");
        assert_eq!(
            click(&reveals, &AllGone),
            Click::Again(b"/gone".to_vec()),
            "the control: a reveal is"
        );
        let rules = &Rules::NONE;
        for asked in [Asked::Link, Asked::Printed(LineCol::default())] {
            let found = on_found(&asked, b"/etc/hosts", Host(""), rules);
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
        let said = c.apply("[link.path]\ncommand = [\"/bin/echo\", \"{path}\"]\n");
        assert!(said.is_empty(), "{said:?}");
        let rules = &c.link.rules;
        let printed = on_found(
            &Asked::Printed(LineCol::default()),
            b"/etc/hosts",
            Host(""),
            rules,
        );
        assert!(
            matches!(printed, Found::Command(Ok(_))),
            "the control: a printed path runs [link.path]'s command: {printed:?}"
        );
        let link = on_found(&Asked::Link, b"/etc/hosts", Host(""), rules);
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
        let said = c.apply(
            "[link.path]\nline_col = \"on\"\naction = \"window\"\n\
             command = [\"/bin/echo\", \"{path}:{line}\"]\n",
        );
        assert!(said.is_empty(), "{said:?}");
        let rules = &c.link.rules;
        let lc = LineCol {
            line: "7".into(),
            col: String::new(),
        };
        let printed = on_found(&Asked::Printed(lc), b"/etc/hosts", Host(""), rules);
        let Found::Window(Ok(run)) = printed else {
            panic!("a window: {printed:?}");
        };
        assert_eq!(run.argv(), ["/bin/echo", "/etc/hosts:7"]);
        let link = on_found(&Asked::Link, b"/etc/hosts", Host(""), rules);
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
        p.set_names(vec![HERE.to_string()]);
        let mut u = Uris::default();
        u.set_host(HERE.to_string());
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
            at().line(Rows::ZERO, baud_domain::Row::ZERO),
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
                &[HERE.to_string()]
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
                Host(HERE),
                &p,
            );
            assert!(h.struck().is_empty(), "{uri}: no chord, no strike");
            let _ = h.update_with(&t, at_cell, at(), Chord::Cmd, Focus::Gained, Host(HERE), &p);
            assert_eq!(h.struck().len(), 1, "{uri}: struck under the chord");
            assert_eq!(h.take_entered(), Some(Gone::File(b"/gone".to_vec())));
            let spot = Spot {
                scrolled: at(),
                back: Rows::ZERO,
                row: Row::ZERO,
                col: Col::at_index(1),
            };
            let link = hovered(&t, spot).expect("the link");
            let bar =
                preview_text_with(&link, &t, Host(HERE), Cols::of_index(58), &p).expect("a bar");
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
        p.set_names(vec![HERE.to_string()]);
        let mut u = Uris::default();
        u.set_host(HERE.to_string());
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
        let t = admit(b"https://a.example/", Host("")).unwrap();
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
        let _ = h.update_with(&t, over, at(), Chord::Cmd, Focus::Gained, Host(""), &p);
        assert_eq!(h.struck().len(), 1, "on: struck under the chord");
        let _ = p.set_missing(Turned::Off);
        let _ = h.update_with(&t, over, at(), Chord::Cmd, Focus::Gained, Host(""), &p);
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
        let plain = admit(uri, Host("")).expect("admitted");
        assert_eq!(
            p.target_mark(&plain),
            Mark::Struck,
            "the control: no browser, struck"
        );
        let copied = crate::link::admit_with(uri, Host(""), &c.link.rules).expect("the rule's");
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
            let t = crate::link::admit_with(b"tel:+15550100", Host(""), &c.link.rules)
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
        p.set_names(vec![HERE.to_string()]);
        let mut u = Uris::default();
        u.set_host(HERE.to_string());
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
        let here = [HERE.to_string()];
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
            let want = admit(file_uri(p.as_bytes()).as_bytes(), Host("")).is_ok();
            assert_eq!(opens_as_uri(p.as_bytes()), want, "{p:?}");
        }
        assert!(opens_as_uri(long.as_bytes()), "the control: at the cap");
        assert!(!opens_as_uri(longer.as_bytes()), "the control: past it");
    }

    /// The click both drivers make on an `OSC 8` link at `line`:
    /// `link::admit_printed` with the line's origin, then `link::click`.
    fn clicked_at(
        uri: &[u8],
        p: &Paths,
        line: AbsLine,
    ) -> Result<crate::link::Click, crate::link::Refused> {
        crate::link::admit_printed(uri, Host(HERE), p.rules(), Some(p.origin(line)))
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
                p.set_names(vec![HERE.to_string()]);
                let line = at().line(Rows::ZERO, baud_domain::Row::ZERO);
                let uri = format!("file://{h}/etc/hosts");
                feed(&mut t, marked(&uri, "hosts").as_bytes());
                // Named as the worker names the read's group.
                p.written_under(front, LineSpan::of(line, line));
                p.set_front(front, line);
                let read = link_at(&t, baud_domain::Row::ZERO);
                assert_eq!(read, uri.as_bytes(), "the bytes the core kept");
                let in_front = clicked_at(&read, &p, line);
                // It ends: the shell is in front again, and the link is
                // clicked from what it printed.
                p.written_under(Front::Local, LineSpan::of(line.below(), line.below()));
                p.set_front(Front::Local, line.below());
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
        p.set_names(vec![HERE.to_string()]);
        let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        feed(&mut t, CAPTURE);
        let last = at().line(Rows::ZERO, t.cursor().0);
        p.written_under(Front::Elsewhere, LineSpan::of(first, last));
        p.set_front(Front::Elsewhere, last);
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
                p.written_under(Front::Local, LineSpan::of(after, after));
                p.set_front(Front::Local, after);
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
    /// `link::admit_printed` with [`Paths::printed_origin`], then
    /// `link::click`.
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
        let line = at().line(Rows::ZERO, row);
        let clicked =
            crate::link::admit_printed(&uri, Host(HERE), p.rules(), p.printed_origin(&under, line))
                .map(|target| crate::link::click(&target, p));
        (uri, clicked)
    }

    /// **A `file://` URI a remote program printed as plain text is refused
    /// here, as a marked one is**: `file://localhost/etc/passwd` on a line
    /// `ssh` wrote, read from the text of the cells, is refused while `ssh`
    /// is in front and after it ends; on a line the shell wrote it is
    /// revealed, the control. Named break: give a link read from the text
    /// no origin in `printed_origin` -- the remote line's is revealed.
    #[test]
    fn a_plain_file_uri_printed_under_ssh_is_refused_here() {
        use crate::link::{Click, Refused};
        let run = |front: Front| {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            p.set_names(vec![HERE.to_string()]);
            let line = at().line(Rows::ZERO, baud_domain::Row::ZERO);
            feed(&mut t, b"file://localhost/etc/passwd\r\n");
            p.written_under(front, LineSpan::of(line, line));
            p.set_front(front, line);
            let (uri, in_front) = clicked_under(&t, &p, baud_domain::Row::ZERO);
            assert_eq!(uri, b"file://localhost/etc/passwd", "read from the text");
            p.written_under(Front::Local, LineSpan::of(line.below(), line.below()));
            p.set_front(Front::Local, line.below());
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
    /// `printed_origin` -- a plain link from `ssh` reveals, or opens its
    /// window.
    #[test]
    fn a_plain_file_uri_takes_its_origin_as_a_marked_one_does() {
        use crate::link::{Click, Refused};
        let mut window = crate::config::Config::defaults();
        assert!(
            window
                .apply("[link.file]\naction = \"window\"\ncommand = [\"hx\", \"{url}\"]\n")
                .is_empty(),
            "the rule reads"
        );
        let hosts = ["", "localhost", HERE, "other.example"];
        let fronts = [Front::Local, Front::Elsewhere, Front::Multiplexer];
        for rules in [None, Some(window.link.rules.clone())] {
            for h in hosts {
                for front in fronts {
                    let mut owned = Owned::new(&WS);
                    let mut t = Term::new(owned.storage(), &WS);
                    let mut p = Paths::new(None, Forms::DEFAULT);
                    p.set(Turned::On);
                    p.set_names(vec![HERE.to_string()]);
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
                    p.written_under(front, LineSpan::of(first, second));
                    p.set_front(front, second);
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
                    p.written_under(Front::Local, LineSpan::of(back, back));
                    p.set_front(Front::Local, back);
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
    /// `printed_origin` -- it is revealed before its line is named.
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
        p.written_under(Front::Local, LineSpan::of(line, line));
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
    /// `printed_origin` -- the three are revealed.
    #[test]
    fn a_real_ssh_sessions_plain_file_uris_are_its_machines() {
        use crate::link::Refused;
        const CAPTURE: &[u8] = include_bytes!("../../tests/ssh-link/remote-plain-file-uris.raw");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set(Turned::On);
        p.set_names(vec![HERE.to_string()]);
        let first = at().line(Rows::ZERO, baud_domain::Row::ZERO);
        feed(&mut t, CAPTURE);
        let last = at().line(Rows::ZERO, t.cursor().0);
        p.written_under(Front::Elsewhere, LineSpan::of(first, last));
        p.set_front(Front::Elsewhere, last);
        let want = [
            ("file:///etc/passwd", Refused::Remote),
            ("file://localhost/etc/passwd", Refused::Remote),
            ("file://here.example/etc/passwd", Refused::Remote),
            ("file://remotebox/etc/hosts", Refused::Host),
        ];
        let after = last.below();
        for ended in [false, true] {
            if ended {
                p.written_under(Front::Local, LineSpan::of(after, after));
                p.set_front(Front::Local, after);
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
        p.set_names(vec![HERE.to_string()]);
        let (l0, l1) = (
            at().line(Rows::ZERO, baud_domain::Row::ZERO),
            at().line(Rows::ZERO, baud_domain::Row::from_index(1)),
        );
        feed(
            &mut t,
            marked("file://here.example/etc/hosts", "hosts").as_bytes(),
        );
        feed(&mut t, b"\r\n");
        p.written_under(Front::Local, LineSpan::of(l0, l0));
        feed(
            &mut t,
            marked("file://localhost/etc/hosts", "hosts").as_bytes(),
        );
        p.written_under(Front::Elsewhere, LineSpan::of(l1, l1));
        p.set_front(Front::Elsewhere, l1);
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
        p.set_names(vec![HERE.to_string()]);
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
        remote.set_names(vec![HERE.to_string()]);
        p.written_under(Front::Local, LineSpan::of(line, line));
        assert_eq!(
            clicked_at(&read, &p, line),
            Ok(Click::Reveal),
            "named local"
        );
        remote.written_under(Front::Elsewhere, LineSpan::of(line, line));
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
        let said = c.apply(
            "[link]\npaths = \"off\"\n\n[link.file]\naction = \"window\"\n\
             command = [\"/usr/bin/true\", \"{url}\"]\n",
        );
        assert!(said.is_empty(), "{said:?}");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut p = Paths::new(None, Forms::DEFAULT);
        p.set_rules(c.link.rules.clone());
        p.set_names(vec![HERE.to_string()]);
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
        p.written_under(Front::Elsewhere, LineSpan::of(l0, l0));
        p.written_under(Front::Local, LineSpan::of(l1, l1));
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

    /// **Spans of one origin that touch are one, and a span gone from
    /// history goes**: adjacent local reads merge, a remote one between
    /// stays its own, and trimming at a line drops only what ends above it.
    /// Named break: never trim -- the dropped span still answers.
    #[test]
    fn line_spans_merge_and_leave_with_history() {
        let mut p = Paths::new(None, Forms::DEFAULT);
        let l = |n: usize| at().line(Rows::ZERO, baud_domain::Row::from_index(n));
        p.written_under(Front::Local, LineSpan::of(l(0), l(0)));
        p.written_under(Front::Local, LineSpan::of(l(1), l(2)));
        p.written_under(Front::Elsewhere, LineSpan::of(l(3), l(3)));
        p.written_under(Front::Local, LineSpan::of(l(3), l(3)));
        assert_eq!(
            p.spans.len(),
            3,
            "the two local reads merged: {:?}",
            p.spans
        );
        assert_eq!(
            p.origin(l(3)),
            Origin::Remote,
            "a line both wrote is remote"
        );
        p.trim_spans(l(3));
        assert_eq!(p.origin(l(1)), Origin::Unknown, "gone with its history");
        assert_eq!(p.origin(l(3)), Origin::Remote);
    }
}
