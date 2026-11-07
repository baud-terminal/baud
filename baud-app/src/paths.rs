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
//! apart: the rows that read wrote are above the line, so they get none. A
//! `./` on the alternate screen gets none, since a full-screen program has a
//! folder of its own.
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
#![forbid(unsafe_code)]

use core::ops::Range;
use std::collections::HashMap;

use baud_core::{FileHost, Screen, Term};
use baud_domain::{Cols, Rows};
use baud_render::{AbsLine, Scrolled, row_of};

use crate::lined::{Reader, Text, Uris};
use crate::link::{Edge, Ix, MAX, Unit};
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
    } else if u(0).is_some_and(|&x| part(x) == Part::In && !is(Some(&x), '/')) {
        Form::Bare
    } else {
        return None;
    };
    (forms.of(form) == Turned::On).then_some(form)
}

/// **The paths in a line's text**, each once, in order: from a start
/// [`start_at`] reads, over path characters, to the first that ends one, its
/// trailing prose cut off. A path the walk's end touches, one past `MAX`
/// characters, one with a character it must not hold, or a bare one with no
/// `/` inside or a `:` (`host:path` names another machine) is none.
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
        while end < n {
            match part(units[end]) {
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
    geometry: Option<(Cols, Rows)>,
    known: HashMap<Vec<u8>, State>,
    generation: Generation,
    owed: Owed,
    collect: Option<Collect>,
    reads: Reads,
    /// What is in front, and the last line written while what is in front
    /// now, or anything not known to be local, was.
    front: Front,
    elsewhere_through: Option<AbsLine>,
    multiplexed_through: Option<AbsLine>,
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
        self.collect = None;
        self.owed = Owed::No;
        self.generation = self.generation.next();
        self.update_reads();
    }

    /// [`Reads`] again, after the marks, the collection or the answers
    /// changed: once a batch, never a frame.
    fn update_reads(&mut self) {
        let draws = self.known.values().any(|s| *s == State::Is(Probe::Exists));
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
        match self.known.get(path) {
            Some(State::Is(p)) => Some(*p),
            Some(State::Asked) | None => None,
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

    /// **Rows to `through` were written while `front` was in front**, named
    /// by the worker after the program had gone: they keep no mark if it is
    /// not local, and no `./` under a multiplexer. What is in front now is
    /// [`Paths::set_front`]'s, and does not change.
    pub fn written_under(&mut self, front: Front, through: AbsLine) {
        let (before, mux) = (self.elsewhere_through, self.multiplexed_through);
        match front {
            Front::Elsewhere => self.elsewhere_through = before.max(Some(through)),
            Front::Multiplexer => self.multiplexed_through = mux.max(Some(through)),
            Front::Local => {}
        }
        if (before, mux) != (self.elsewhere_through, self.multiplexed_through) {
            self.generation = self.generation.next();
        }
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
            if let Some(s) = self.known.get_mut(&path) {
                if *s != State::Is(probe) {
                    *s = State::Is(probe);
                    moved = Moved::Yes;
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
        let (first, last) = text.lines;
        if self.marks != Marks::On
            || self.front == Front::Elsewhere
            || self.elsewhere_through.is_some_and(|t| last <= t)
        {
            return false;
        }
        if !form.relative() {
            return true;
        }
        text.screen == Screen::Normal
            && self.front != Front::Multiplexer
            && !self.multiplexed_through.is_some_and(|t| first <= t)
            && self.dir.local.is_some()
            && self.dir.from.is_some_and(|f| first >= f)
    }

    /// The path `token` names, resolved, into `self.key`; false when it
    /// cannot be: no home, no folder.
    fn resolve(&mut self, units: &[Unit], token: &Token) -> bool {
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
                let Some(d) = &self.dir.local else {
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
    fn read(&mut self, text: Text<'_>, found: &mut Vec<Range<usize>>) -> Looked {
        let mut toks = core::mem::take(&mut self.tokens);
        tokens(text.units, (text.head, text.tail), &self.forms, &mut toks);
        let uris = found.len();
        let mut looked = 0;
        for t in &toks {
            let shown = t.span.start < text.shown.1 && text.shown.0 < t.span.end;
            let over = found[..uris]
                .iter()
                .any(|f| f.start < t.span.end && t.span.start < f.end);
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
            if !self.resolve(text.units, t) {
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
                        Some(State::Asked) | None if c.queue.len() < BATCH_MAX => {
                            c.kept.insert(self.key.clone(), State::Asked);
                            c.queue.push(Candidate {
                                path: self.key.clone(),
                                form: t.form,
                                line: text.lines.1,
                            });
                        }
                        Some(State::Asked) | None => c.over = Over::Yes,
                    }
                }
            }
            match state {
                Some(State::Is(Probe::Exists)) if opens_as_uri(&self.key) => {
                    found.push(t.span.clone());
                }
                Some(State::Is(_)) | Some(State::Asked) => {}
                None if self.collect.is_none() => self.owed = Owed::Yes,
                None => {}
            }
        }
        self.tokens = toks;
        Looked(looked)
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
                key.extend_from_slice(self.dir.local.as_deref()?);
                if key.last() != Some(&b'/') {
                    key.push(b'/');
                }
                push_utf8(below_folder(t.form, body), &mut key);
            }
        }
        let probe = match self.known.get(&key) {
            Some(State::Is(p @ (Probe::Exists | Probe::Missing))) => *p,
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

    fn read(&mut self, text: Text<'_>, found: &mut Vec<Range<usize>>) -> usize {
        let mut n = 0;
        if self.urls == Turned::On {
            n += self.uris.read(text, found);
        }
        if self.paths.reads == Reads::Yes {
            n += self.paths.read(text, found).0;
            found.sort_unstable_by_key(|r| r.start);
        }
        n
    }

    fn admits(&mut self, uri: &[u8]) -> bool {
        self.urls == Turned::On && self.uris.admits(uri)
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
}
