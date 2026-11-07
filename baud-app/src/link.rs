//! Link activation: which link a Cmd-click on macOS or a Ctrl-click on
//! Linux opens, and which it refuses -- an `OSC 8` run, or a URI printed as
//! plain text.
//!
//! The core stores a link per cell and answers `Term::link_uri`; whether a
//! click on it opens anything is this app's policy rather than the
//! protocol's, so it lives here and not in the core or the domain -- and it
//! is pure over bytes, so its tests run on the Linux lint too. Three parts:
//!
//! * [`admit`] decides whether a URI may be handed to the OS at all, and
//!   what for. An allowlist, because every registered scheme handler on
//!   macOS is an application `NSWorkspace` *launches*, and a program's
//!   output is attacker-controlled: `http`, `https` and `mailto` are
//!   opened; `file` on this host is *revealed* in Finder, which launches
//!   nothing (`file:///Applications/Foo.app` handed to the opener would
//!   launch Foo); everything else is refused, and the refusal is named so
//!   the log line can say why.
//! * [`find`] reads a URI out of the text around a cell that is not in an
//!   `OSC 8` run (`[[source: rfc3986]]`): from one of the four schemes
//!   above, over RFC 3986's characters, across the rows a soft wrap joined
//!   and no further, and only when the click asks -- [`under`] runs it on
//!   an unarmed Cmd press (Ctrl on Linux) and on the release of a press it
//!   armed, never on a drag; a move and a frame read it under the pointer
//!   ([`Hover`]), bounded by `LINK_MAX`. Where the RFC is silent,
//!   practice decides: trailing `.,;:!?'*` and an unmatched closing bracket
//!   are prose, not the URI; an opening `(` or `[` just before the scheme
//!   ends it at the closer that matches nothing inside; a letter outside
//!   ASCII running on from it refuses it, because the text names a page
//!   `admit` cannot open as shown. A URI that may have been cut -- past
//!   `LINK_MAX`, a `%` without two hex digits, a bracket outside an
//!   IP-literal host, or a `<` or `"` opened before it and never closed
//!   before a hard line break (Appendix C's own example) -- is refused
//!   whole, never opened shortened.
//! * [`gesture`] decides what a mouse event over a link means: Cmd-click
//!   (Ctrl-click on Linux, where the Linux driver opens with `xdg-open` and
//!   reveals through the file manager, `opener.rs` and `bus.rs`),
//!   armed on the press and opened on the release over the same link, the
//!   whole gesture the terminal's -- a child tracking the mouse never sees
//!   it, the way Shift already carves a selection out.
//!
//! * [`Hover`] shows where a link goes before it is opened, while the
//!   gesture's modifier is held over it: its cells underlined and its
//!   address in a bar ([`preview`]), escaped a byte at a time
//!   ([`escaped`]), shortened to keep the end of its host ([`shortened`]),
//!   and with the reason in words when a click will not open it; and the
//!   pointer a pointing hand while a click would open it ([`Hover::pointer`]).
//!
//! * A file path printed as plain text is a link once the worker found the
//!   file (`paths.rs`, `probe.rs`): [`Under::Path`], read through a
//!   [`PathsAt`] the driver hands in. A click on one asks again, off the
//!   frame, and goes through [`admit`] as a `file://` URI, unchanged.
//!
//! * Your `[link.*]` rules (`rules.rs`) widen the list and nothing else:
//!   [`admit_with`] lets a scheme a rule names through, the first rule in the
//!   file deciding what a click does, after every check above and with no
//!   part that starts with `-`; [`find_with`] reads its links in the text as
//!   it reads baud's own four. An `OSC 8` cell is its run's and no plain-text
//!   rule's ([`unit`] reads it as a gap), so a cell belongs to one link.
//!
//! * The line at rest, the scan of the rows a frame changed, is `lined.rs`'s;
//!   it reads URIs through [`found_in`], [`find`]'s own rules, so every
//!   refusal above holds at rest as at a click: a URI refused is drawn as no
//!   link. [`Hover::over`] is the look of the link under the pointer.
//!
//! What this does not do, stated: a URI outside such a wrapper broken
//! across a hard line break is read only up to the break. It gets no line
//! where that break is at the row's last column and the next row carries on in
//! a URI's characters ([`may_be_cut`]), the mark of a program wrapping by
//! itself; a click there still opens what is read, and the bar shows it.
#![forbid(unsafe_code)]

use baud_core::{Focus, LINK_MAX, PointerShape, Term};

#[cfg(test)]
pub use crate::paths::NoPaths;
pub use crate::paths::{Gone, PathsAt};
use crate::rules::{NO_RULES, Rules};

use baud_domain::{Attrs, Codepoint, Col, Cols, LinkEpoch, LinkId, PackedCell, Row, Rows, Wrap};
use baud_render::{
    AbsLine, Field, Hints, HoverPaint, LinkHover, LinkLook, LinkSpan, Point, Scrolled, row_of,
    row_styles_of,
};
use core::ops::Range;

/// What an admitted target is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Hand it to the default handler for its scheme: the browser, the
    /// mail composer -- or, for `https`, whatever LaunchServices routes it
    /// to, universal-link applications included. That is an accepted risk
    /// rather than a closed one.
    Open,
    /// Select it in Finder. Never launches.
    Reveal,
    /// Run the program a `[link.*]` rule of yours names, with no shell
    /// (`rules.rs`, `Rules::command`).
    Command,
    /// Put the link's text on the clipboard, as a copy of a selection is,
    /// and open nothing: a `[link.*]` rule's `action = "copy"`.
    Copy,
}

/// The four schemes the policy knows, whose spellings are [`KNOWN`]'s, the one
/// list [`admit`] and [`find`] read; and a scheme a `[link.*]` rule of yours
/// added, which only [`admit_with`] takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scheme {
    Http,
    Https,
    Mailto,
    File,
    Added,
}

/// Why a URI was not handed to the OS: the first rule it failed, in the
/// order the rules are checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refused {
    /// Empty, or past the core's own cap. The core refuses to store a URI
    /// past `LINK_MAX` now, so the second cannot arrive; the rule stands so
    /// the two caps cannot drift apart unnoticed.
    Length,
    /// A byte outside printable ASCII: a space, a control, DEL, a high
    /// byte. RFC 3986 URLs are ASCII, and the parser lets DEL and high
    /// bytes through.
    Byte,
    /// Not `http`, `https`, `mailto` or `file`, nor a scheme a `[link.*]`
    /// rule of yours names. A leading `-` fails here too, which is what
    /// closes the argv hazard an opener would have had.
    Scheme,
    /// `http`/`https` with no authority, `mailto` with no address, `file`
    /// on another host or without an absolute path.
    Host,
    /// A `mailto` header other than `subject` or `body`: `bcc=` prefills a
    /// hidden recipient, and any other header reaches whatever application
    /// is registered for `mailto`, which is not necessarily a composer.
    Header,
    /// A URI read from the text that RFC 3986 would not take whole: a `%`
    /// not followed by two hex digits (§ 2.1), a bracket anywhere but
    /// around an IP-literal host (§ 3.2.2), or a letter outside ASCII
    /// running on from it. Opening what is left would open a cut address.
    Syntax,
    /// A printed path, or a `file://` link, the worker found gone: never
    /// [`admit`]'s, which reads no file system; a click asks again before
    /// it says so.
    NotFound,
    /// A link a `[link.*]` rule routes with a part that starts with `-`, or
    /// `%2D`: the host, user or port, or what follows the scheme's `:`
    /// (`rules::dashless`), which a program handed the link may read as an
    /// option -- CVE-2017-1000117's shape.
    Dash,
    /// A scheme a `[link.*]` rule of yours turns off (`action = "off"`), one
    /// of baud's own four included.
    Off,
    /// No app for the link's kind, as the worker last found: never
    /// [`admit`]'s either, and a click still tries.
    NoApp(Scheme),
    /// No program for the command a `[link.NAME]` rule runs, as the worker
    /// last found: a click still tries.
    NoCommand,
}

/// A URI the policy admitted, with what it is for. Only [`admit`] builds
/// one, so the opener's call sites cannot be handed a raw string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target<'a> {
    uri: &'a str,
    scheme: Scheme,
    /// The `[link.*]` rule that took it, by its place, and what it does.
    rule: Option<(crate::rules::RuleIx, Action)>,
}

impl<'a> Target<'a> {
    pub fn as_str(&self) -> &'a str {
        self.uri
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    pub fn action(&self) -> Action {
        match (self.rule, self.scheme) {
            (Some((_, act)), _) => act,
            (None, Scheme::File) => Action::Reveal,
            (None, Scheme::Http | Scheme::Https | Scheme::Mailto | Scheme::Added) => Action::Open,
        }
    }

    /// The `[link.*]` rule that took it, by its place in the rules.
    pub fn rule(&self) -> Option<crate::rules::RuleIx> {
        self.rule.map(|(ix, _)| ix)
    }
}

/// This machine's name, for a `file://host/path` link -- the form
/// `ls --hyperlink` emits -- to be recognised as local. Empty when unknown,
/// which admits only the empty and `localhost` hosts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Host<'a>(pub &'a str);

/// The longest URI the policy admits: the core's own cap, so a URI the
/// core stored whole is never refused for length here.
pub const MAX: usize = LINK_MAX;

/// Decide whether `uri` may be handed to the OS, and what for, by baud's own
/// list alone.
pub fn admit<'a>(uri: &'a [u8], host: Host<'_>) -> Result<Target<'a>, Refused> {
    admit_with(uri, host, &NO_RULES)
}

/// **[`admit`], with your `[link.*]` rules**: a scheme a rule names is let
/// through, the first rule in the file naming it deciding what a click
/// does. Nothing a rule says loosens the rest: the bytes and the length are
/// checked first; a default scheme keeps its own rules whichever handler
/// takes it -- `file` names this machine or none, `mailto` carries only
/// `subject` and `body`; and a link a rule routes has no part that starts
/// with `-` ([`crate::rules::dashless`]).
pub fn admit_with<'a>(uri: &'a [u8], host: Host<'_>, rules: &Rules) -> Result<Target<'a>, Refused> {
    if uri.is_empty() || uri.len() > MAX {
        return Err(Refused::Length);
    }
    if !uri.iter().all(|b| (0x21..=0x7e).contains(b)) {
        return Err(Refused::Byte);
    }
    // Every byte is printable ASCII, so this cannot fail; the arm is what
    // the type system asks for rather than a second rule.
    let s = core::str::from_utf8(uri).map_err(|_| Refused::Byte)?;
    let colon = s.find(':').ok_or(Refused::Scheme)?;
    let (name, rest) = (&s[..colon], &s[colon + 1..]);
    let ruled = match rules.rule_for(name) {
        Some((_, r)) if *r.act() == crate::rules::Act::Off => return Err(Refused::Off),
        Some((ix, r)) => Some((
            ix,
            match r.act() {
                crate::rules::Act::Open if r.scheme() == "file" => Action::Reveal,
                crate::rules::Act::Open | crate::rules::Act::Off => Action::Open,
                crate::rules::Act::Command(_) => Action::Command,
                crate::rules::Act::Copy => Action::Copy,
            },
        )),
        None => None,
    };
    let scheme = match (
        KNOWN.iter().find(|k| name.eq_ignore_ascii_case(k.name)),
        ruled,
    ) {
        (Some(k), _) => k.scheme,
        (None, Some(_)) => Scheme::Added,
        (None, None) => return Err(Refused::Scheme),
    };
    if ruled.is_some() {
        crate::rules::dashless(rest)?;
    }
    match scheme {
        Scheme::Added => {
            if rest.is_empty() {
                return Err(Refused::Host);
            }
        }
        Scheme::Http | Scheme::Https => {
            let auth = rest.strip_prefix("//").ok_or(Refused::Host)?;
            let end = auth.find(['/', '?', '#']).unwrap_or(auth.len());
            if end == 0 {
                return Err(Refused::Host);
            }
        }
        Scheme::Mailto => {
            let (addr, query) = match rest.find('?') {
                Some(q) => (&rest[..q], Some(&rest[q + 1..])),
                None => (rest, None),
            };
            if addr.is_empty() {
                return Err(Refused::Host);
            }
            if let Some(q) = query {
                for pair in q.split('&') {
                    let key = pair.split('=').next().unwrap_or("");
                    if !(key.eq_ignore_ascii_case("subject") || key.eq_ignore_ascii_case("body")) {
                        return Err(Refused::Header);
                    }
                }
            }
        }
        Scheme::File => {
            let auth = rest.strip_prefix("//").ok_or(Refused::Host)?;
            let end = auth.find('/').ok_or(Refused::Host)?;
            let h = &auth[..end];
            let local = h.is_empty()
                || h.eq_ignore_ascii_case("localhost")
                || h.eq_ignore_ascii_case(host.0);
            if !local {
                return Err(Refused::Host);
            }
        }
    }
    Ok(Target {
        uri: s,
        scheme,
        rule: ruled,
    })
}

/// Whether `name` is one of the four schemes baud opens by default, which
/// its own list finds in the text; a rule naming one changes only what a
/// click does.
pub(crate) fn known_scheme(name: &str) -> bool {
    KNOWN.iter().any(|k| name.eq_ignore_ascii_case(k.name))
}

/// One RFC 3986 character class, a bit of [`CLASSES`]' byte per ASCII code.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Class(u8);

impl Class {
    const NONE: Class = Class(0);
    /// unreserved / reserved / `%`: a character a URI may contain (§ 2).
    pub(crate) const URI: Class = Class(1);
    /// ALPHA / DIGIT / `+` / `-` / `.`: a scheme's (§ 3.1).
    pub(crate) const SCHEME: Class = Class(2);
    /// HEXDIG, what follows a `%` (§ 2.1).
    const HEX: Class = Class(4);
    /// Prose punctuation the RFC allows inside a URI and practice reads as
    /// the sentence's when it ends one: `.,;:!?'*`.
    const TRAIL: Class = Class(8);

    pub(crate) fn has(self, k: Class) -> bool {
        self.0 & k.0 == k.0
    }
}

/// `unreserved` (§ 2.3) besides ALPHA and DIGIT.
const UNRESERVED: &[u8] = b"-._~";
/// `gen-delims` (§ 2.2).
const GEN_DELIMS: &[u8] = b":/?#[]@";
/// `sub-delims` (§ 2.2).
const SUB_DELIMS: &[u8] = b"!$&'()*+,;=";
const SCHEME_MARKS: &[u8] = b"+-.";
const TRAILING: &[u8] = b".,;:!?'*";

/// Whether byte `$b` is in the set `$set`, at compile time.
macro_rules! member {
    ($set:expr, $b:expr) => {{
        let set: &[u8] = $set;
        let mut j = 0;
        let mut hit = false;
        while j < set.len() {
            if set[j] == $b {
                hit = true;
            }
            j += 1;
        }
        hit
    }};
}

/// The class of every ASCII code, built at compile time; a code past the
/// table is no class at all, which is what a URI makes of it (§ 2).
const CLASSES: [Class; 128] = classes();

const fn classes() -> [Class; 128] {
    let mut t = [Class::NONE; 128];
    let mut b: u8 = 0;
    let mut i = 0;
    while i < t.len() {
        let alnum = b.is_ascii_alphanumeric();
        let mut bits = 0;
        if alnum
            || b == b'%'
            || member!(UNRESERVED, b)
            || member!(GEN_DELIMS, b)
            || member!(SUB_DELIMS, b)
        {
            bits |= Class::URI.0;
        }
        if alnum || member!(SCHEME_MARKS, b) {
            bits |= Class::SCHEME.0;
        }
        if b.is_ascii_hexdigit() {
            bits |= Class::HEX.0;
        }
        if member!(TRAILING, b) {
            bits |= Class::TRAIL.0;
        }
        t[i] = Class(bits);
        b = b.wrapping_add(1);
        i += 1;
    }
    t
}

/// A cell as the URI reader sees it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    /// A character drawn alone in its cell.
    Char(char),
    /// A character with combining marks attached: never ASCII, and a
    /// letter as far as the end of a URI goes.
    Marked,
    /// Nothing that reads as text: an empty cell, a cell in an `OSC 8` run,
    /// a concealed one -- what opens is what is seen, and is not another
    /// link's.
    Gap,
}

pub(crate) fn class(u: Unit) -> Class {
    match u {
        Unit::Char(c) => usize::try_from(u32::from(c))
            .ok()
            .and_then(|i| CLASSES.get(i))
            .copied()
            .unwrap_or(Class::NONE),
        Unit::Marked | Unit::Gap => Class::NONE,
    }
}

/// A letter or digit outside ASCII, or a marked cell: text a URI runs on
/// into, as the WHATWG URL Standard reads it (`[[source: whatwg-url]]`,
/// "URL code points" take everything from U+00A0 on).
fn letter(u: Unit) -> bool {
    match u {
        Unit::Char(c) => !c.is_ascii() && c.is_alphanumeric(),
        Unit::Marked => true,
        Unit::Gap => false,
    }
}

/// Whether a scheme's URI has an authority, which is where brackets may
/// stand (§ 3.2.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Authority {
    Yes,
    No,
}

/// A scheme `admit` lets through, as it starts a URI in text.
pub(crate) struct Known {
    /// What [`admit`] takes, and how a URI in the text opens with it.
    scheme: Scheme,
    name: &'static str,
    /// What follows the name in the text: a URI with an authority is only
    /// that URI past `//` (§ 3.2).
    marks: &'static str,
    authority: Authority,
}

impl Known {
    /// How many characters the name and its marks take.
    fn prefix_len(&self) -> Width {
        Width(self.name.len() + self.marks.len())
    }

    fn prefix(&self) -> impl Iterator<Item = char> + '_ {
        self.name.chars().chain(self.marks.chars())
    }
}

/// **The schemes, the one list**: [`admit`] reads the names and [`find`] the
/// prefixes, so a scheme one knows the other cannot miss.
const KNOWN: [Known; 4] = [
    Known {
        scheme: Scheme::Https,
        name: "https",
        marks: "://",
        authority: Authority::Yes,
    },
    Known {
        scheme: Scheme::Http,
        name: "http",
        marks: "://",
        authority: Authority::Yes,
    },
    Known {
        scheme: Scheme::Mailto,
        name: "mailto",
        marks: ":",
        authority: Authority::No,
    },
    Known {
        scheme: Scheme::File,
        name: "file",
        marks: "://",
        authority: Authority::Yes,
    },
];

/// **Whether `tail`, the last eight ASCII bytes read, lowered, the newest in
/// its low byte, ends in a known scheme's name and `:`**: what a line must
/// hold before it is worth reading for a URI at all, so a scan reads a line
/// with none as one compare a cell.
/// A scheme a rule of yours added is one more compare at a `:`, and none
/// anywhere else ([`Rules::ends`]).
pub(crate) fn scheme_ends(tail: crate::rules::Tail, rules: &Rules) -> bool {
    let crate::rules::Tail(tail) = tail;
    if tail & 0xFF != u64::from(b':') {
        return false;
    }
    KNOWN.iter().any(|k| {
        let n = k.name.len();
        let packed = k.name.bytes().fold(0u64, |a, b| (a << 8) | u64::from(b));
        (tail >> 8) & ((1u64 << (8 * n)) - 1) == packed
    }) || rules.ends(crate::rules::Tail(tail))
}

/// **Where a scheme starts a URI in the text**: how many units its name and
/// marks take, and whether an authority follows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Start {
    prefix: Width,
    authority: Authority,
}

/// **How many units a scheme's name and marks take** where it starts a URI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Width(usize);

impl Start {
    fn of(k: &Known) -> Start {
        Start {
            prefix: k.prefix_len(),
            authority: k.authority,
        }
    }
}

/// The scheme `rest` starts with, compared without case (§ 3.1): one of
/// baud's own, by its spelling and marks, or one a rule of yours added,
/// whose `//` after the `:` says an authority follows (§ 3.2).
pub(crate) fn scheme_at(rest: &[Unit], rules: &Rules) -> Option<Start> {
    let known = KNOWN.iter().find(|k| {
        k.prefix_len().0 <= rest.len()
            && k.prefix()
                .zip(rest)
                .all(|(c, u)| matches!(u, Unit::Char(x) if x.eq_ignore_ascii_case(&c)))
    });
    if let Some(k) = known {
        return Some(Start::of(k));
    }
    let n = rules.added_at(rest)?.scheme().len();
    let slashes = matches!(
        rest.get(n + 1..n + 3),
        Some([Unit::Char('/'), Unit::Char('/')])
    );
    Some(match slashes {
        true => Start {
            prefix: Width(n + 3),
            authority: Authority::Yes,
        },
        false => Start {
            prefix: Width(n + 1),
            authority: Authority::No,
        },
    })
}

/// A position in a [`Window`]'s text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ix(pub(crate) usize);

/// Whether a [`Window`]'s end is the logical line's, or where the walk
/// stopped because a URI there would be past `LINK_MAX` anyway.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Edge {
    #[default]
    Line,
    Cut,
}

/// The text around a clicked cell: the logical line's cells, as far as
/// `MAX + 1` either side, each with the cell it was read from.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Window {
    text: Vec<Unit>,
    cells: Vec<Point>,
    at: Ix,
    head: Edge,
    tail: Edge,
}

/// A URI found in a [`Window`]: its units, and whether it may be opened.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Found {
    span: Range<usize>,
    verdict: Result<(), Refused>,
}

/// Where the closer of a `(` or `[` just before the scheme falls: the
/// first `close` in `uri` that matches no `open` inside it.
fn first_unmatched(uri: &[Unit], open: char, close: char) -> Option<Ix> {
    let mut depth = 0;
    for (i, u) in uri.iter().enumerate() {
        if *u == Unit::Char(open) {
            depth += 1;
        } else if *u == Unit::Char(close) {
            if depth == 0 {
                return Some(Ix(i));
            }
            depth -= 1;
        }
    }
    None
}

/// How many more closers than openers `uri` holds: past zero a trailing
/// closer closes something outside it.
fn outnumbered(uri: &[Unit], open: char, close: char) -> isize {
    let (mut opens, mut closes) = (0, 0);
    for u in uri {
        if *u == Unit::Char(open) {
            opens += 1;
        } else if *u == Unit::Char(close) {
            closes += 1;
        }
    }
    closes - opens
}

fn bracket(u: &Unit) -> bool {
    matches!(u, Unit::Char('[' | ']'))
}

/// RFC 3986 on a URI's characters, whole: every `%` begins `pct-encoded`
/// (§ 2.1), and a bracket stands only around an IP-literal host (§ 3.2.2).
fn syntax(uri: &[Unit], known: Start) -> Result<(), Refused> {
    for (i, u) in uri.iter().enumerate() {
        if *u == Unit::Char('%') {
            let hex = |u: Option<&Unit>| u.is_some_and(|u| class(*u).has(Class::HEX));
            if !(hex(uri.get(i + 1)) && hex(uri.get(i + 2))) {
                return Err(Refused::Syntax);
            }
        }
    }
    let rest = &uri[known.prefix.0..];
    let after = match known.authority {
        Authority::No => rest,
        Authority::Yes => {
            let end = rest
                .iter()
                .position(|u| matches!(u, Unit::Char('/' | '?' | '#')))
                .unwrap_or(rest.len());
            let auth = &rest[..end];
            let at = auth
                .iter()
                .rposition(|u| *u == Unit::Char('@'))
                .map_or(0, |i| i + 1);
            if auth[..at].iter().any(bracket) {
                return Err(Refused::Syntax);
            }
            let host = &auth[at..];
            if host.first() == Some(&Unit::Char('[')) {
                let close = host
                    .iter()
                    .position(|u| *u == Unit::Char(']'))
                    .ok_or(Refused::Syntax)?;
                let port = &host[close + 1..];
                let port_ok = port.is_empty() || port.first() == Some(&Unit::Char(':'));
                if host[1..close].iter().any(bracket) || !port_ok || port.iter().any(bracket) {
                    return Err(Refused::Syntax);
                }
            } else if host.iter().any(bracket) {
                return Err(Refused::Syntax);
            }
            &rest[end..]
        }
    };
    match after.iter().any(bracket) {
        true => Err(Refused::Syntax),
        false => Ok(()),
    }
}

/// The URI around the clicked unit of `w`, if one is there.
///
/// RFC 3986 decides what a URI is: it starts at one of `admit`'s schemes
/// where a scheme may start -- after a character that cannot continue a
/// scheme, so `xhttps://h` is scheme `xhttps` and none of these (§ 3.1) --
/// and runs over the RFC's characters (§ 2); Appendix C's wrappers, the
/// `"` and `<>` around one, are not among them and so are never part of
/// it. Practice decides what the RFC leaves open (the module doc). The
/// first URI that starts at or before the click and reaches it is the
/// one: a URI inside another's query is the other's. The drivers read
/// through [`find_with`], so only tests call it.
#[cfg(test)]
pub fn find(w: &Window) -> Option<Found> {
    find_with(w, &NO_RULES)
}

/// [`find`], a scheme a rule of yours added found as baud's own are.
pub fn find_with(w: &Window, rules: &Rules) -> Option<Found> {
    let text = &w.text[..];
    let at = w.at.0;
    if !class(*text.get(at)?).has(Class::URI) {
        return None;
    }
    let mut lo = at;
    while lo > 0 && class(text[lo - 1]).has(Class::URI) {
        lo -= 1;
    }
    let mut hi = at + 1;
    while hi < text.len() && class(text[hi]).has(Class::URI) {
        hi += 1;
    }
    found_in(text, (lo, hi), Ix(lo), Ix(at), (w.head, w.tail), rules)
}

/// [`find`] over `text`, the run of URI characters around `at` being `lo..hi`
/// and the units before `from` already read: the scan at rest (`lined.rs`)
/// reads a run from where the last URI ended, one pass, so a run of many
/// costs what one does.
pub(crate) fn found_in(
    text: &[Unit],
    (lo, hi): (usize, usize),
    from: Ix,
    at: Ix,
    (head, tail): (Edge, Edge),
    rules: &Rules,
) -> Option<Found> {
    let (Ix(from), Ix(at)) = (from, at);
    let cut = (lo == 0 && head == Edge::Cut) || (hi == text.len() && tail == Edge::Cut);
    for p in from..=at {
        if p > 0 && class(text[p - 1]).has(Class::SCHEME) {
            continue;
        }
        let Some(known) = scheme_at(&text[p..hi], rules) else {
            continue;
        };
        let body = p + known.prefix.0;
        if body >= hi {
            continue;
        }
        if cut {
            return Some(Found {
                span: lo..hi,
                verdict: Err(Refused::Length),
            });
        }
        let before = p.checked_sub(1).map(|q| text[q]);
        let opened = matches!(before, Some(Unit::Char('<' | '"')));
        let blank = |u: &Unit| matches!(u, Unit::Gap | Unit::Char(' '));
        if opened && tail == Edge::Line && text[hi..].iter().all(blank) {
            return Some(Found {
                span: p..hi,
                verdict: Err(Refused::Length),
            });
        }
        let mut end = hi;
        let mut by_run = true;
        for (open, close) in [('(', ')'), ('[', ']')] {
            if before != Some(Unit::Char(open)) {
                continue;
            }
            if let Some(Ix(e)) = first_unmatched(&text[p..hi], open, close) {
                end = end.min(p + e);
                by_run = false;
            }
        }
        // Counted once and kept as the end moves, so a run of closers costs
        // one pass and not one a closer.
        let mut rounds = outnumbered(&text[p..end], '(', ')');
        let mut squares = outnumbered(&text[p..end], '[', ']');
        while end > body {
            let last = text[end - 1];
            let unmatched =
                (last == Unit::Char(')') && rounds > 0) || (last == Unit::Char(']') && squares > 0);
            if class(last).has(Class::TRAIL) || unmatched {
                if last == Unit::Char(')') {
                    rounds -= 1;
                } else if last == Unit::Char(']') {
                    squares -= 1;
                }
                end -= 1;
            } else {
                break;
            }
        }
        if end <= body || at >= end {
            continue;
        }
        let runs_on = by_run && text.get(hi).is_some_and(|u| letter(*u));
        let verdict = match runs_on {
            true => Err(Refused::Syntax),
            false => syntax(&text[p..end], known),
        };
        return Some(Found {
            span: p..end,
            verdict,
        });
    }
    None
}

impl Found {
    /// The units it spans, and whether RFC 3986 takes it whole.
    pub(crate) fn parts(&self) -> (Range<usize>, Result<(), Refused>) {
        (self.span.clone(), self.verdict)
    }
}

/// **Whether a URI ending at `to` may have been cut by the program that wrote
/// it**: its last cell is the row's last, the row ends there by the writer's
/// own line break and not a soft wrap, and the next row starts in a URI's
/// characters. A program that wraps by itself leaves exactly this, and a line
/// under the first row's part would call a cut address whole; a click still
/// reads what is written and the bar shows it. What this cannot see, stated: a
/// program that wraps short of the last column, as a pane inside a
/// multiplexer does.
pub(crate) fn may_be_cut(term: &Term<'_>, scrolled: Scrolled, to: Point) -> bool {
    let Some((cells, wrap)) = row_of(term, scrolled, to.line) else {
        return false;
    };
    if to.col.index() + 1 < cells.len() || wrap == Wrap::Continues {
        return false;
    }
    let below = to.line.below();
    let Some((next, _)) = row_of(term, scrolled, below) else {
        return false;
    };
    next.first().is_some_and(|&cell| {
        let styles = row_styles_of(term, scrolled, below);
        class(unit(term, styles, cell)).has(Class::URI)
    })
}

/// A URI read from the text: the cells it spans, both ends inclusive, by
/// absolute line so a scroll between the press and the release keeps it,
/// and its bytes, or why it may not be opened.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Bare {
    from: Point,
    to: Point,
    text: Vec<u8>,
    verdict: Result<(), Refused>,
}

impl Bare {
    /// The URI to admit, or why it is refused before `admit` sees it.
    pub fn uri(&self) -> Result<&[u8], Refused> {
        self.verdict.map(|()| &*self.text)
    }

    /// The text as read, for the log line a refusal writes.
    pub fn text(&self) -> &[u8] {
        &self.text
    }
}

impl Window {
    /// The URI around the clicked cell, with its cells. The drivers read
    /// through [`Window::bare_with`], so only tests call it.
    #[cfg(test)]
    pub fn bare(&self) -> Option<Bare> {
        self.bare_in(&mut Vec::new(), &NO_RULES)
    }

    /// [`Window::bare`], a scheme a rule of yours added read too.
    pub fn bare_with(&self, rules: &Rules) -> Option<Bare> {
        self.bare_in(&mut Vec::new(), rules)
    }

    /// [`Window::bare`], its bytes in `text`'s buffer, taken only for a URI.
    fn bare_in(&self, text: &mut Vec<u8>, rules: &Rules) -> Option<Bare> {
        let f = find_with(self, rules)?;
        let (from, to) = (
            *self.cells.get(f.span.start)?,
            *self.cells.get(f.span.end.checked_sub(1)?)?,
        );
        text.clear();
        text.extend(self.text[f.span.clone()].iter().filter_map(|u| match u {
            Unit::Char(c) => u8::try_from(*c).ok(),
            Unit::Marked | Unit::Gap => None,
        }));
        Some(Bare {
            from,
            to,
            text: core::mem::take(text),
            verdict: f.verdict,
        })
    }
}

/// What a cell reads as: its character, unless it carries marks, is
/// empty, is in an `OSC 8` run or is concealed.
pub(crate) fn unit(term: &Term<'_>, styles: baud_domain::RowStyles<'_>, cell: PackedCell) -> Unit {
    // Through the row's own table: a line the core froze off the screen
    // names its ids in a generation's words (`Term::style_in`).
    if term.link_uri(cell.flags.link()).is_some()
        || term.style_in(styles, cell).attrs().has(Attrs::HIDDEN)
    {
        return Unit::Gap;
    }
    if term.cluster_marks(cell).next().is_some() {
        return Unit::Marked;
    }
    term.base_of(cell)
        .as_codepoint()
        .and_then(|cp| char::from_u32(cp.value()))
        .map_or(Unit::Gap, Unit::Char)
}

/// The logical line around column `col` of absolute line `line`: the
/// cells before it back to where the writer last ended a row, and from it
/// on to where the writer next ended one, `MAX + 1` either side at most.
/// A wide character's tail adds nothing; a click on one is a click on its
/// lead. Bounded by `LINK_MAX`, not by the line: a URI longer than that is
/// refused whatever is past it, so a line a megabyte long costs a click
/// what a short one does.
pub fn window(term: &Term<'_>, scrolled: Scrolled, line: AbsLine, col: Col) -> Option<Window> {
    let mut read = Reading::default();
    read_window(term, scrolled, line, col, &mut read)?;
    Some(read.window)
}

/// **The buffers a read of the link under the pointer keeps**, for
/// [`Hover::update`], which reads again at each new byte with the pointer
/// still: grown to the longest line read, then allocating nothing.
#[derive(Default)]
struct Reading {
    left: Vec<(Unit, Point)>,
    right: Vec<(Unit, Point)>,
    window: Window,
    text: Vec<u8>,
    spans: Vec<LinkSpan>,
    struck: Vec<baud_render::LinkLine>,
}

/// [`window`], into `read`'s window.
fn read_window(
    term: &Term<'_>,
    scrolled: Scrolled,
    line: AbsLine,
    col: Col,
    read: &mut Reading,
) -> Option<()> {
    let (row, wrap) = row_of(term, scrolled, line)?;
    let mut c = col.index().min(row.len().checked_sub(1)?);
    if c > 0 && row[c].content.is_wide_tail() {
        c -= 1;
    }
    let Reading {
        left,
        right,
        window,
        ..
    } = read;
    left.clear();
    right.clear();
    let mut head = Edge::Line;
    let (mut l, mut cells, mut i) = (line, row, c);
    'left: loop {
        let styles = row_styles_of(term, scrolled, l);
        while i > 0 {
            i -= 1;
            if cells[i].content.is_wide_tail() {
                continue;
            }
            if left.len() > MAX {
                head = Edge::Cut;
                break 'left;
            }
            left.push((
                unit(term, styles, cells[i]),
                Point::new(l, Col::at_index(i)),
            ));
        }
        let Some(up) = l.above() else { break };
        match row_of(term, scrolled, up) {
            Some((prev, Wrap::Continues)) => {
                (l, cells, i) = (up, prev, prev.len());
            }
            _ => break,
        }
    }
    let mut tail = Edge::Line;
    let (mut l, mut cells, mut wraps, mut i) = (line, row, wrap, c);
    'right: loop {
        let styles = row_styles_of(term, scrolled, l);
        while i < cells.len() {
            if !cells[i].content.is_wide_tail() {
                if right.len() > MAX {
                    tail = Edge::Cut;
                    break 'right;
                }
                right.push((
                    unit(term, styles, cells[i]),
                    Point::new(l, Col::at_index(i)),
                ));
            }
            i += 1;
        }
        if wraps != Wrap::Continues {
            break;
        }
        let down = l.below();
        let Some((next, w)) = row_of(term, scrolled, down) else {
            break;
        };
        (l, cells, wraps, i) = (down, next, w, 0);
    }
    window.at = Ix(left.len());
    window.text.clear();
    window.cells.clear();
    for &(u, p) in left.iter().rev().chain(right.iter()) {
        window.text.push(u);
        window.cells.push(p);
    }
    window.head = head;
    window.tail = tail;
    Some(())
}

/// Where a pointer event fell: the view the user sees, `back` lines up the
/// history, and the cell in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spot {
    pub scrolled: Scrolled,
    pub back: Rows,
    pub row: Row,
    pub col: Col,
}

/// The link under the pointer, as [`gesture`] needs it: the `OSC 8` link
/// of the cell, which is a lookup, or else -- on an unarmed Cmd press in
/// a key window, and on the release of a press armed over text -- the URI
/// [`find`] reads around it. No other event reads the text for a click;
/// the preview reads it on a move only while the modifier is held
/// ([`hovered`]), bounded by `LINK_MAX` as a click's read is ([`window`]).
/// The drivers read through [`under_with`], so only tests call it.
#[cfg(test)]
pub fn under(
    term: &Term<'_>,
    spot: Spot,
    press: Press,
    chord: Chord,
    focus: Focus,
    armed: Option<&Armed>,
) -> Option<Under> {
    under_with(term, spot, press, chord, focus, armed, &NoPaths)
}

/// [`under`], a printed path read through `paths` where the text holds no
/// URI: one the worker found, or found gone, which the click asks about
/// again.
pub fn under_with(
    term: &Term<'_>,
    spot: Spot,
    press: Press,
    chord: Chord,
    focus: Focus,
    armed: Option<&Armed>,
    paths: &dyn PathsAt,
) -> Option<Under> {
    let view = term.view(spot.back);
    let cell = *view.row(spot.row).get(spot.col.index())?;
    let link = Osc8::of(term, cell);
    if link.uri(term).is_some() {
        return Some(Under::Osc8(link));
    }
    let read = match (press, armed) {
        (Press::Down, None) => chord == Chord::Cmd && focus == Focus::Gained,
        (Press::Up, Some(a)) => matches!(a.link, Under::Bare(_) | Under::Path(_)),
        _ => false,
    };
    if !read {
        return None;
    }
    let line = spot.scrolled.line(spot.back, spot.row);
    let w = window(term, spot.scrolled, line, spot.col)?;
    match w.bare_with(paths.rules()) {
        Some(b) => Some(Under::Bare(b)),
        None => printed_in(term, &w, paths).map(Under::Path),
    }
}

/// **A printed path under a click or the pointer**: the path at the window's
/// cell that `paths` has an answer for, its cells, and the URI a click hands
/// [`admit`]. One the worker found gone is refused as [`Refused::NotFound`],
/// so the bar says so and nothing opens.
fn printed_in(term: &Term<'_>, w: &Window, paths: &dyn PathsAt) -> Option<Printed> {
    let lines = (w.cells.first()?.line, w.cells.last()?.line);
    let at = paths.at(&w.text, w.at, (w.head, w.tail), lines, term.screen())?;
    let from = *w.cells.get(at.span.start)?;
    let to = *w.cells.get(at.span.end.checked_sub(1)?)?;
    let uri = crate::paths::file_uri(&at.path);
    let verdict = match at.probe {
        crate::paths::Probe::Exists => admit(uri.as_bytes(), Host("")).map(|_| ()),
        crate::paths::Probe::Missing | crate::paths::Probe::Unknown => Err(Refused::NotFound),
    };
    Some(Printed {
        from,
        to,
        path: at.path,
        form: at.form,
        line_col: at.line_col,
        uri,
        verdict,
    })
}

/// **A file path printed as plain text, as a click or the preview holds it**:
/// its cells, the path as resolved, and its `file://` URI, which is the
/// address the bar shows and the one `admit` is handed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Printed {
    from: Point,
    to: Point,
    path: Vec<u8>,
    form: crate::paths::Form,
    /// The `:line:col` printed after it, for a command of yours.
    line_col: crate::paths::LineCol,
    uri: String,
    verdict: Result<(), Refused>,
}

impl Printed {
    /// The `:line:col` printed after it, empty where none was or
    /// `[link.path] line_col` is off.
    pub fn line_col(&self) -> &crate::paths::LineCol {
        &self.line_col
    }

    /// The path as resolved, which a click asks the worker about again.
    pub fn path(&self) -> &[u8] {
        &self.path
    }

    /// How it was printed, which says whether a multiplexer hides it.
    pub fn form(&self) -> crate::paths::Form {
        self.form
    }

    /// The line it ends on, which says which program wrote it.
    pub fn line(&self) -> AbsLine {
        self.to.line
    }

    /// The URI a click hands `admit`, once the worker found the file again.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn uri(&self) -> &str {
        &self.uri
    }
}

/// What a press is over: an `OSC 8` run, a URI in the text, or a file path
/// printed in it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Under {
    Osc8(Osc8),
    Bare(Bare),
    Path(Printed),
}

/// **An `OSC 8` link as a gesture holds it**: the cell's index, and the
/// link table's epoch it was read under.
///
/// The core frees a slot once nothing names it and gives it to the next
/// target (`Term::link_epoch`), so an index kept from a press to its
/// release, or under a held preview, can come to name another link's
/// target while the cell it was read from is gone. It resolves only while
/// the epoch is the one it was read under, and two are the same link only
/// if both agree: a release over a cell that took the freed slot is not
/// the link that was pressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Osc8 {
    id: LinkId,
    epoch: LinkEpoch,
}

impl Osc8 {
    /// The link `cell` names, as read now.
    pub fn of(term: &Term<'_>, cell: PackedCell) -> Osc8 {
        Osc8 {
            id: cell.flags.link(),
            epoch: term.link_epoch(),
        }
    }

    /// Its target, while no sweep has freed a slot since it was read.
    pub fn uri<'a>(&self, term: &'a Term<'_>) -> Option<&'a [u8]> {
        match term.link_epoch() == self.epoch {
            true => term.link_uri(self.id),
            false => None,
        }
    }
}

/// A mouse event's kind, as the gesture reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    Down,
    Dragged,
    Up,
    Exited,
    Other,
}

/// The modifier chord of a press: Cmd alone is the link gesture -- Ctrl on
/// Linux -- and Shift wins over it -- one meaning per modifier, and
/// Shift's is the selection carve-out, which is older and documented.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chord {
    Cmd,
    Other,
}

/// The click count of a press: a repeat is a double or triple click, which
/// arms so its release is swallowed too and never opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clicks {
    Single,
    Repeat,
}

/// A press the terminal took for itself: the link under it, and whether
/// the release over the same link opens it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Armed {
    pub link: Under,
    pub clicks: Clicks,
}

/// What a mouse event means while a link gesture may be in progress.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Gesture {
    /// Not the terminal's: the child's tracking or the selection takes it.
    Pass,
    /// A Cmd press on a link: the gesture is the terminal's from here.
    Arm(Armed),
    /// Swallow this event and stay armed.
    Hold,
    /// Swallow this event and disarm; nothing opens.
    Drop,
    /// Swallow this event, disarm, and open the link.
    Open(Under),
}

/// The gesture, as a pure function of the event, the app's focus, what is
/// armed and the link under the pointer. Arm only on a Cmd press over a
/// link while the window is key -- a click into a background window is the
/// one that would open something the user did not intend, and AppKit does
/// not deliver it anyway until the view accepts first mouse, which is the
/// crate change recorded beside hover. Open on the release over the *same
/// link*, whatever modifiers the release carries: a press that drifts a
/// point across a cell boundary inside the link, or from a wide lead onto
/// its tail, still opens; a release over another link, or off one, drops.
/// A URI in the text is the same link when it is read back at the release
/// from the same cells with the same bytes, so text rewritten under a held
/// press opens nothing.
pub fn gesture(
    press: Press,
    chord: Chord,
    clicks: Clicks,
    focus: Focus,
    armed: Option<&Armed>,
    under: Option<&Under>,
) -> Gesture {
    match (armed, press) {
        (None, Press::Down) => match (chord, focus, under) {
            (Chord::Cmd, Focus::Gained, Some(link)) => Gesture::Arm(Armed {
                link: link.clone(),
                clicks,
            }),
            _ => Gesture::Pass,
        },
        (None, _) => Gesture::Pass,
        (Some(_), Press::Dragged | Press::Other) => Gesture::Hold,
        (Some(a), Press::Up) => match (a.clicks, under) {
            (Clicks::Single, Some(l)) if *l == a.link => Gesture::Open(l.clone()),
            _ => Gesture::Drop,
        },
        (Some(_), Press::Exited | Press::Down) => Gesture::Drop,
    }
}

// ---- seeing where a link goes ---------------------------------------------

/// **The address a link preview shows**, a byte at a time: printable ASCII
/// as it is, a backslash doubled, and every other byte -- a space, a
/// control, DEL, any byte from `0x80` -- as `\xNN`. A URI is "a sequence
/// of characters from a very limited set: the letters of the basic Latin
/// alphabet, digits, and a few special characters" (`[[source: rfc3986]]`
/// 1.2.1), and [`admit`] opens nothing else, so what is drawn is exactly
/// the bytes and nothing shown can pass for an address that opens: no
/// control can move or end the line, no bidi control can reorder it
/// (`[[source: uax9-tr9-52]]`), and no letter can be a look-alike of
/// another script's (`[[source: utr36-15]]` 2.5, 2.3).
pub fn escaped(uri: &[u8]) -> String {
    let mut out = String::with_capacity(uri.len());
    for &b in uri {
        match b {
            b'\\' => out.push_str("\\\\"),
            0x21..=0x7e => out.push(char::from(b)),
            _ => out.push_str(&format!("\\x{b:02X}")),
        }
    }
    out
}

/// What the bar says of a printed path the worker found gone.
#[cfg(target_os = "macos")]
const NOT_FOUND: &str = "no such file on this Mac";
#[cfg(not(target_os = "macos"))]
const NOT_FOUND: &str = "no such file on this computer";

/// What a refusal says in the preview, before a click: the rule it fails,
/// in words.
pub fn refusal_words(why: Refused) -> &'static str {
    match why {
        Refused::Length => "too long",
        Refused::Byte => "not a plain address",
        Refused::Scheme => "not a web, mail or file link",
        Refused::Host => "no host, or a file on another computer",
        Refused::Header => "a mail link with extra headers",
        Refused::Syntax => "not a whole address",
        Refused::NotFound => NOT_FOUND,
        Refused::Dash => "a part that starts with -",
        Refused::Off => "off in your config",
        Refused::NoApp(Scheme::Http) => "no app for http:",
        Refused::NoApp(Scheme::Https) => "no app for https:",
        Refused::NoApp(Scheme::Mailto) => "no app for mailto:",
        Refused::NoApp(Scheme::File) => "no app for file:",
        Refused::NoApp(Scheme::Added) => "no app for it",
        Refused::NoCommand => "no app for its command",
    }
}

/// The ellipsis a shortened address shows where it left text out.
const ELLIPSIS: char = '\u{2026}';

/// **An escaped address shortened to `room` columns**, keeping what a spoof
/// cannot push off the row: the scheme, then the host -- whole if it fits,
/// else its end, where the registrable domain is, after an ellipsis -- then
/// the end of the rest after an ellipsis. A host that starts like another
/// site's (`accounts.example.com.aaaa....evil.test`) shows the part that
/// names where it goes.
pub fn shortened(address: &str, room: Cols) -> String {
    let room = room.index();
    let chars: Vec<char> = address.chars().collect();
    if chars.len() <= room {
        return address.to_string();
    }
    // The scheme and its `//`, the host up to the first `/`, `?` or `#`,
    // and the rest; an address without a scheme is all host.
    let scheme =
        chars
            .iter()
            .position(|&c| c == ':')
            .map_or(0, |i| match chars.get(i + 1..i + 3) {
                Some(['/', '/']) => i + 3,
                _ => i + 1,
            });
    let host_end = chars[scheme..]
        .iter()
        .position(|c| matches!(c, '/' | '?' | '#'))
        .map_or(chars.len(), |i| scheme + i);
    let (head, host, rest) = (
        &chars[..scheme],
        &chars[scheme..host_end],
        &chars[host_end..],
    );
    let take_end =
        |s: &[char], n: usize| -> String { s[s.len() - n.min(s.len())..].iter().collect() };
    let head_s: String = head.iter().collect();
    // Too narrow for a scheme and anything after it: the address's end.
    if room <= head.len() + 2 {
        return format!("{ELLIPSIS}{}", take_end(&chars, room.saturating_sub(1)));
    }
    let after = room - head.len();
    // The rest keeps at least a quarter of the row's room, so where the
    // path ends is in sight too.
    let tail = rest.len().min(room / 4);
    let host_s: String = host.iter().collect();
    if host.len() + 1 + tail <= after && !rest.is_empty() {
        return format!(
            "{head_s}{host_s}{ELLIPSIS}{}",
            take_end(rest, after - host.len() - 1)
        );
    }
    match rest.is_empty() {
        true => format!("{head_s}{ELLIPSIS}{}", take_end(host, after - 1)),
        false => {
            let host_room = after.saturating_sub(2 + tail).max(1);
            let tail = after.saturating_sub(2 + host_room);
            format!(
                "{head_s}{ELLIPSIS}{}{ELLIPSIS}{}",
                take_end(host, host_room),
                take_end(rest, tail)
            )
        }
    }
}

/// **What the preview's bar says for a link**: its escaped address, after
/// what refuses it in words when a click would open nothing, so the user
/// knows before clicking, shortened to `room` columns. A reason too long
/// for the room is cut to "Won't open". The drivers read through
/// [`preview_text_with`], so only tests call it.
#[cfg(test)]
pub fn preview_text(link: &Under, term: &Term<'_>, host: Host<'_>, room: Cols) -> Option<String> {
    preview_text_with(link, term, host, room, &NoPaths)
}

/// [`preview_text`], with what `paths` found gone said as the reason, and
/// the link admitted with the rules `paths` holds: a link a rule takes
/// shows its address as any other does.
pub fn preview_text_with(
    link: &Under,
    term: &Term<'_>,
    host: Host<'_>,
    room: Cols,
    paths: &dyn PathsAt,
) -> Option<String> {
    let room = room.index();
    let rules = paths.rules();
    let (uri, verdict) = verdict(link, term, host, paths)?;
    let address = escaped(uri);
    // A click that copies says so: it opens nothing.
    let copies = match link {
        Under::Path(_) => *rules.path_act() == crate::rules::PathAct::Copy,
        Under::Osc8(_) | Under::Bare(_) => {
            admit_with(uri, host, rules).is_ok_and(|t| t.action() == Action::Copy)
        }
    };
    let label = match (verdict, link) {
        (Ok(()), _) if copies => "Copies  ".to_string(),
        // A path names no machine in its address: the bar says which one it
        // was found on.
        (Ok(()), Under::Path(_)) => format!("On {}  ", crate::paths::MACHINE),
        (Ok(()), _) => String::new(),
        (Err(why), _) => {
            let long = format!("Won't open, {}  ", refusal_words(why));
            match long.len() + 12 <= room {
                true => long,
                false => "Won't open  ".to_string(),
            }
        }
    };
    let left = room.saturating_sub(label.len());
    Some(format!(
        "{label}{}",
        shortened(&address, Cols::of_index(left))
    ))
}

/// **The bytes a link shows and whether a click opens it**: an `OSC 8`
/// link's URI, or the text of a URI read from the cells, and [`admit`]'s
/// answer for it, then what `paths` found gone of one it admits; none for
/// an `OSC 8` slot whose link is gone.
fn verdict<'a>(
    link: &'a Under,
    term: &'a Term<'_>,
    host: Host<'_>,
    paths: &dyn PathsAt,
) -> Option<(&'a [u8], Result<(), Refused>)> {
    let rules = paths.rules();
    let admitted = |uri: &[u8]| {
        let t = admit_with(uri, host, rules)?;
        match paths.gone(&t) {
            Some(g) => Err(gone_refusal(&g)),
            None => Ok(()),
        }
    };
    Some(match link {
        Under::Osc8(link) => {
            let uri = link.uri(term)?;
            (uri, admitted(uri))
        }
        Under::Bare(b) => (b.text(), b.uri().and_then(admitted)),
        Under::Path(p) => (p.uri.as_bytes(), p.verdict),
    })
}

/// What a link found gone is refused as, in the bar.
fn gone_refusal(g: &Gone) -> Refused {
    match g {
        Gone::File(_) => Refused::NotFound,
        Gone::NoApp(s) => Refused::NoApp(*s),
        Gone::NoCommand => Refused::NoCommand,
    }
}

/// **What the worker found gone of `link`**: a printed path's file, or what
/// `paths` says of a link `admit` lets through.
pub fn gone_of(link: &Under, term: &Term<'_>, host: Host<'_>, paths: &dyn PathsAt) -> Option<Gone> {
    match link {
        Under::Path(p) => match p.verdict {
            Err(Refused::NotFound) => Some(Gone::File(p.path.clone())),
            _ => None,
        },
        Under::Osc8(o) => paths.gone(&admit_with(o.uri(term)?, host, paths.rules()).ok()?),
        Under::Bare(b) => paths.gone(&admit_with(b.uri().ok()?, host, paths.rules()).ok()?),
    }
}

/// What a click on a link `admit_with` let through does.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Click {
    /// A rule's `copy`: the link's text to the clipboard.
    Copy,
    /// A rule's command, with the link.
    Command,
    /// Its file, which the worker found gone, asked about again, as a
    /// printed path's click is: revealed if it came back
    /// (`paths::on_found`), said if not.
    Again(Vec<u8>),
    /// The system's app for its kind.
    Open,
    /// Shown in the file manager.
    Reveal,
}

/// **What a click on `target` does**: a rule's copy or command is the
/// link's own action and runs whatever the worker found, and so is an
/// open; a link revealed whose file the worker found gone is asked about
/// again, and only that, so no found file is opened by the system. Both
/// drivers dispatch on this alone, so neither puts the asking ahead of a
/// rule.
pub fn click(target: &Target<'_>, paths: &dyn PathsAt) -> Click {
    match target.action() {
        Action::Copy => Click::Copy,
        Action::Command => Click::Command,
        Action::Open => Click::Open,
        Action::Reveal => match paths.gone(target) {
            Some(Gone::File(path)) => Click::Again(path),
            _ => Click::Reveal,
        },
    }
}

/// The link under the pointer while the link gesture's modifier is held:
/// the cell's `OSC 8` link, or the URI the text around it reads as. Read
/// on every move while the modifier is held, bounded as a click's read is
/// ([`window`]). The drivers read through [`Hover`], so only tests call it.
#[cfg(test)]
pub fn hovered(term: &Term<'_>, spot: Spot) -> Option<Under> {
    hovered_in(term, spot, &mut Reading::default(), &NoPaths)
}

/// [`hovered`], in `read`'s buffers, a printed path read through `paths`
/// where the text holds no URI.
fn hovered_in(
    term: &Term<'_>,
    spot: Spot,
    read: &mut Reading,
    paths: &dyn PathsAt,
) -> Option<Under> {
    let view = term.view(spot.back);
    let cell = *view.row(spot.row).get(spot.col.index())?;
    let link = Osc8::of(term, cell);
    if link.uri(term).is_some() {
        return Some(Under::Osc8(link));
    }
    let line = spot.scrolled.line(spot.back, spot.row);
    read_window(term, spot.scrolled, line, spot.col, read)?;
    match read.window.bare_in(&mut read.text, paths.rules()) {
        Some(b) => Some(Under::Bare(b)),
        None => printed_in(term, &read.window, paths).map(Under::Path),
    }
}

/// **The link's cells on the view, a run a row.** An `OSC 8` link is every
/// cell naming its slot -- one URI and one `id` -- connected or not: cells
/// sharing a nonempty `id` "are always underlined together on mouseover",
/// and for cells without one the terminal "does some heuristics"
/// (`[[source: osc8-spec]]`, "Hover underlining and the id parameter"). A
/// URI read from the text is its cells from its first to its last, across
/// the rows a soft wrap joined.
pub fn link_spans(term: &Term<'_>, spot: Spot, link: &Under) -> Vec<LinkSpan> {
    let mut out = Vec::new();
    link_spans_into(term, spot, link, &mut out);
    out
}

/// [`link_spans`], into `out`'s buffer.
fn link_spans_into(term: &Term<'_>, spot: Spot, link: &Under, out: &mut Vec<LinkSpan>) {
    let view = term.view(spot.back);
    let (rows, cols) = (view.rows().index(), view.cols().index());
    out.clear();
    match link {
        // A link whose slot was freed since names none of the cells.
        Under::Osc8(link) if link.uri(term).is_none() => {}
        Under::Osc8(link) => {
            for y in 0..rows {
                let row = view.row(Row::from_index(y));
                let mut run: Option<usize> = None;
                for x in 0..=cols.min(row.len()) {
                    let on = row.get(x).is_some_and(|c| c.flags.link() == link.id);
                    match (on, run) {
                        (true, None) => run = Some(x),
                        (false, Some(a)) => {
                            out.push(LinkSpan {
                                row: Row::from_index(y),
                                from: Col::at_index(a),
                                to: Col::at_index(x - 1),
                            });
                            run = None;
                        }
                        _ => {}
                    }
                }
            }
        }
        Under::Bare(Bare {
            from: b_from,
            to: b_to,
            ..
        })
        | Under::Path(Printed {
            from: b_from,
            to: b_to,
            ..
        }) => {
            let (b_from, b_to) = (*b_from, *b_to);
            for y in 0..rows {
                let line = spot.scrolled.line(spot.back, Row::from_index(y));
                if line < b_from.line || line > b_to.line {
                    continue;
                }
                let from = if line == b_from.line {
                    b_from.col.index()
                } else {
                    0
                };
                let to = if line == b_to.line {
                    b_to.col.index()
                } else {
                    cols.saturating_sub(1)
                };
                // A wide character's tail is under the link with its lead.
                let to = match view.row(Row::from_index(y)).get(to + 1) {
                    Some(c) if c.content.is_wide_tail() => to + 1,
                    _ => to,
                };
                out.push(LinkSpan {
                    row: Row::from_index(y),
                    from: Col::at_index(from),
                    to: Col::at_index(to.min(cols.saturating_sub(1))),
                });
            }
        }
    }
}

/// **Where the bar goes**: on the bottom row from the left edge, as a
/// browser's status bubble sits (`[[source: chromium-status-bubble]]`) and
/// Ghostty's banner (`[[source: ghostty-urlhoverbanner]]`); at the bottom
/// row's right end while the link has cells under it on the left, as both
/// move away from the pointer; on the top row only when the link is under
/// both.
pub fn bar_at(spans: &[LinkSpan], width: Cols, cols: Cols, rows: Rows) -> (Row, Col) {
    let (width, cols, rows) = (width.index(), cols.index(), rows.index());
    let last = Row::from_index(rows.saturating_sub(1));
    let under = |a: usize, b: usize| {
        spans
            .iter()
            .any(|s| s.row == last && s.from.index() < b && s.to.index() >= a)
    };
    let right = cols.saturating_sub(width);
    match (under(0, width), under(right, cols)) {
        (false, _) => (last, Col::ZERO),
        (true, false) => (last, Col::at_index(right)),
        (true, true) => (Row::from_index(0), Col::ZERO),
    }
}

/// **What the preview draws for `link` under `spot`**: its cells underlined
/// where a click opens it -- a link [`admit`] refuses is drawn as no link,
/// and the bar says why -- and the bar with its address, a space either side,
/// in the message pair ([`baud_render::message`]): an opaque box and text no
/// program's colours reach, cell-aligned text a program could copy, which
/// shows only while the chord is held. The drivers read through
/// [`preview_with`], so only tests call it.
#[cfg(test)]
pub fn preview(term: &Term<'_>, spot: Spot, link: &Under, host: Host<'_>) -> Option<LinkLook> {
    preview_with(term, spot, link, host, &NoPaths)
}

/// [`preview`], with what `paths` found gone said as the reason, and the
/// link admitted with the rules `paths` holds: **every rule's link shows
/// its address in the same bar**, which no rule turns off, since it is the
/// check on where a click goes.
pub fn preview_with(
    term: &Term<'_>,
    spot: Spot,
    link: &Under,
    host: Host<'_>,
    paths: &dyn PathsAt,
) -> Option<LinkLook> {
    let view = term.view(spot.back);
    let (rows, cols) = (view.rows().index(), view.cols().index());
    let text = preview_text_with(
        link,
        term,
        host,
        Cols::of_index(cols.saturating_sub(2)),
        paths,
    )?;
    let spans = link_spans(term, spot, link);
    let text: Vec<Codepoint> = format!(" {text} ")
        .chars()
        .map(Codepoint::from_char)
        .collect();
    let (row, col) = bar_at(
        &spans,
        Cols::of_index(text.len()),
        Cols::of_index(cols),
        Rows::of_index(rows),
    );
    let n = text.len();
    let opens = matches!(verdict(link, term, host, paths), Some((_, Ok(()))));
    Some(LinkLook {
        spans: match opens {
            true => spans,
            false => Vec::new(),
        },
        bar: Field {
            row,
            col,
            hints: vec![Hints::MESSAGE; n],
            caret: Col::at_index(col.index() + n),
            text,
        },
    })
}

/// **The link under the pointer with no modifier**: what [`Hover`] draws
/// there, once a driver has told it the config (`[link] urls` and `hover`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AtRest {
    /// Nothing: only the chord shows a link.
    #[default]
    Off,
    Look(HoverPaint),
    /// `[link] urls = "off"` and `paths = "on"`: the look on a printed path
    /// only.
    PathsOnly(HoverPaint),
}

/// Whether a mouse button is down: a drag over a link is a selection being
/// made, and no look is drawn over it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Held {
    #[default]
    No,
    Yes,
}

/// The core's count of bytes taken: a new one is a read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Taken(pub usize);

/// **All a read of the link under the pointer depends on**: a read with the
/// same ones answers the same, so a tick over a still screen reads no text.
/// The bytes the core has taken stand for its grid: nothing moves a cell but
/// a byte, a resize or the window, each of which is here.
#[derive(Clone, PartialEq, Eq, Debug)]
struct ReadOf {
    at: Option<(Row, Col)>,
    scrolled: Scrolled,
    back: Rows,
    geometry: (Cols, Rows),
    chord: Chord,
    focus: Focus,
    bytes: Taken,
    epoch: LinkEpoch,
    host: String,
    /// The printed paths' answers, which move with no byte.
    paths: crate::paths::Generation,
}

impl ReadOf {
    fn matches(
        &self,
        term: &Term<'_>,
        (at, scrolled, chord, focus): (Option<(Row, Col)>, Scrolled, Chord, Focus),
        host: Host<'_>,
        paths: crate::paths::Generation,
    ) -> bool {
        self.at == at
            && self.scrolled == scrolled
            && self.chord == chord
            && self.focus == focus
            && self.back == term.window_back()
            && self.geometry == term.geometry()
            && self.bytes == Taken(term.amplification().0)
            && self.epoch == term.link_epoch()
            && self.host == host.0
            && self.paths == paths
    }
}

/// **The link preview's state**: the cell under the pointer, whatever the
/// modifiers, and what it draws. A driver updates it on each pointer
/// event and modifier change, refreshes it while the pointer is over the
/// text at each frame and, on macOS, at each display-link tick, owing a
/// frame when it changes, and clears it when the window loses the
/// keyboard. Every read builds its [`Spot`] from the view shown then, as
/// a click at the same place would: a window held on its line while
/// output retires lines, or moved by a wheel, shows other lines under a
/// pointer that has not moved. A read repeated over what has not changed
/// reads no text ([`ReadOf`]), since with [`AtRest::Look`] it runs with no
/// modifier, at every tick the pointer rests over the text.
///
/// **Two looks, kept apart.** [`Hover::look`] is the chord's: the bar and the
/// hand, which take the copy signal's bar. [`Hover::over`] is the look of the
/// link under the pointer, with or without the chord, only where a click would
/// open it and it is drawn whole; it has no bar, and asks nothing of the
/// pointer's shape.
#[derive(Default)]
pub struct Hover {
    at: Option<(Row, Col)>,
    look: Option<LinkLook>,
    opens: Opens,
    left: LeftPress,
    over: Option<LinkHover>,
    /// The link under the pointer struck while the chord shows it gone: a
    /// printed path, or a `file://` link naming no machine, which are not
    /// struck at rest.
    struck: Vec<baud_render::LinkLine>,
    /// The last link found gone under the pointer, and what to ask again
    /// for it once, when the pointer came over it.
    gone_link: Option<Under>,
    entered: Option<Gone>,
    at_rest: AtRest,
    held: Held,
    read: Option<ReadOf>,
    reads: baud_render::Tally,
    buffers: Reading,
}

/// **Who a left press where the pointer rests would go to**: the cells' --
/// the link gesture's, a selection's or the program's -- or the
/// scrollbar's, whose strip lies over the grid's last columns and takes a
/// left press while its thumb shows (`bar::left_press`), so a click there
/// scrolls rather than opens the link under it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LeftPress {
    #[default]
    Cells,
    Scrollbar,
}

/// Whether a click on the link the preview shows opens it: one [`admit`]
/// refuses opens nothing, and the bar says why.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Opens {
    /// No preview.
    #[default]
    No,
    Yes,
    Refused,
}

impl Hover {
    /// The pointer over screen cell `at` (none off the text), the
    /// driver's count of lines retired `scrolled`, with `chord` held, in a
    /// window with `focus`: the link there previewed while the chord is
    /// the link gesture's and the window is key, and drawn in the config's
    /// look ([`AtRest`]) where a click would open it, with or without the
    /// chord, while no button is down. Whether what the frame draws changed.
    /// The drivers call [`Hover::update_with`], so only tests call it.
    #[cfg(test)]
    pub fn update(
        &mut self,
        term: &Term<'_>,
        at: Option<(Row, Col)>,
        scrolled: Scrolled,
        chord: Chord,
        focus: Focus,
        host: Host<'_>,
    ) -> bool {
        self.update_with(term, at, scrolled, chord, focus, host, &NoPaths)
    }

    /// [`Hover::update`], with the printed paths `paths` has answers for
    /// read as links: read again when their answers move, with no byte.
    #[allow(clippy::too_many_arguments)]
    pub fn update_with(
        &mut self,
        term: &Term<'_>,
        at: Option<(Row, Col)>,
        scrolled: Scrolled,
        chord: Chord,
        focus: Focus,
        host: Host<'_>,
        paths: &dyn PathsAt,
    ) -> bool {
        self.at = at;
        let inputs = (at, scrolled, chord, focus);
        let generation = paths.generation();
        if self
            .read
            .as_ref()
            .is_some_and(|r| r.matches(term, inputs, host, generation))
        {
            return false;
        }
        let mut kept = self.read.take().map_or_else(String::new, |r| r.host);
        kept.clear();
        kept.push_str(host.0);
        self.read = Some(ReadOf {
            at,
            scrolled,
            back: term.window_back(),
            geometry: term.geometry(),
            chord,
            focus,
            bytes: Taken(term.amplification().0),
            epoch: term.link_epoch(),
            host: kept,
            paths: generation,
        });
        let previewing = chord == Chord::Cmd && focus == Focus::Gained;
        let (paint, every) = match (self.at_rest, self.held, focus) {
            (AtRest::Look(paint), Held::No, Focus::Gained) => (Some(paint), true),
            (AtRest::PathsOnly(paint), Held::No, Focus::Gained) => (Some(paint), false),
            _ => (None, false),
        };
        let mut struck = core::mem::take(&mut self.buffers.struck);
        struck.clear();
        let mut gone_link = None;
        let found = at
            .filter(|_| previewing || paint.is_some())
            .and_then(|(row, col)| {
                let s = spot_now(term, scrolled, row, col)?;
                self.reads.bump();
                let link = hovered_in(term, s, &mut self.buffers, paths)?;
                let opens = matches!(verdict(&link, term, host, paths), Some((_, Ok(()))));
                // Gone: under the chord its cells are struck, whether or not
                // they are at rest, and the bar says why; nothing is struck
                // while `missing` is off.
                if let Some(g) = gone_of(&link, term, host, paths).filter(|_| paths.strikes()) {
                    if previewing && drawn_whole(term, s, &link) {
                        link_spans_into(term, s, &link, &mut self.buffers.spans);
                        struck_runs(term, s, &self.buffers.spans, &mut struck);
                    }
                    gone_link = Some((link.clone(), g));
                }
                let kind = every || matches!(link, Under::Path(_));
                let over = paint
                    .filter(|_| kind && opens && drawn_whole(term, s, &link))
                    .map(|paint| {
                        link_spans_into(term, s, &link, &mut self.buffers.spans);
                        LinkHover {
                            spans: core::mem::take(&mut self.buffers.spans),
                            paint,
                        }
                    });
                let look = match previewing {
                    true => preview_with(term, s, &link, host, paths),
                    false => None,
                };
                let opens = match (look.is_some(), opens) {
                    (true, true) => Opens::Yes,
                    (true, false) => Opens::Refused,
                    (false, _) => Opens::No,
                };
                if let Under::Bare(b) = link {
                    self.buffers.text = b.text;
                }
                Some((look, opens, over))
            });
        let (look, opens, over) = found.unwrap_or((None, Opens::No, None));
        let changed =
            look != self.look || opens != self.opens || over != self.over || struck != self.struck;
        self.look = look;
        self.opens = opens;
        self.buffers.struck = core::mem::replace(&mut self.struck, struck);
        match gone_link {
            Some((link, g)) => {
                if self.gone_link.as_ref() != Some(&link) {
                    self.entered = Some(g);
                }
                self.gone_link = Some(link);
            }
            None => self.gone_link = None,
        }
        if let Some(old) = core::mem::replace(&mut self.over, over) {
            self.buffers.spans = old.spans;
        }
        changed
    }

    /// **What the config asks of the link under the pointer**, from the
    /// config as it is read and read again.
    pub fn set_at_rest(&mut self, at_rest: AtRest) {
        if self.at_rest != at_rest {
            self.at_rest = at_rest;
            self.read = None;
        }
    }

    /// Whether a mouse button is down.
    pub fn set_held(&mut self, held: Held) {
        if self.held != held {
            self.held = held;
            self.read = None;
        }
    }

    /// **The look of the link under the pointer**, or none.
    pub fn over(&self) -> Option<&LinkHover> {
        self.over.as_ref()
    }

    /// How many times this has read the text under the pointer: what a
    /// repeated read over a still screen must not add to.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn reads(&self) -> baud_render::Tally {
        self.reads
    }

    /// A frame under a pointer that has not moved: the link under it read
    /// again on the view shown now, with `chord` the modifiers held now --
    /// a read or a wheel may have changed what is under it, a press of
    /// the modifier no event reported shows the preview, and a release
    /// takes it away. Whether what the frame draws changed. The drivers call
    /// [`Hover::refresh_with`], so only tests call it.
    #[cfg(test)]
    pub fn refresh(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        chord: Chord,
        focus: Focus,
        host: Host<'_>,
    ) -> bool {
        self.refresh_with(term, scrolled, chord, focus, host, &NoPaths)
    }

    /// [`Hover::refresh`], with `paths`' printed paths read as links.
    pub fn refresh_with(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        chord: Chord,
        focus: Focus,
        host: Host<'_>,
        paths: &dyn PathsAt,
    ) -> bool {
        match self.at {
            Some(at) => self.update_with(term, Some(at), scrolled, chord, focus, host, paths),
            None => false,
        }
    }

    /// **Who a left press where the pointer rests goes to now**, after a
    /// pointer event or a frame -- the strip under the pointer, the thumb
    /// shown or gone -- and whether that changed, which may change the
    /// pointer ([`Hover::pointer`]).
    pub fn set_left_press(&mut self, left: LeftPress) -> bool {
        core::mem::replace(&mut self.left, left) != left
    }

    /// Nothing previewed, until the next update or refresh: the chord's look,
    /// which a key takes away. The look with no modifier stays under a resting
    /// pointer, and goes with the keyboard at the next read. Whether what the
    /// frame draws changed.
    pub fn clear(&mut self) -> bool {
        self.opens = Opens::No;
        self.read = None;
        let struck = !self.struck.is_empty();
        self.struck.clear();
        self.look.take().is_some() || struck
    }

    /// **The link under the pointer struck for the next frame**: only while
    /// the chord shows it gone.
    pub fn struck(&self) -> &[baud_render::LinkLine] {
        &self.struck
    }

    /// **What to ask again, once, for a link found gone the pointer came
    /// over** (`paths.rs`): its file, or the programs. A read of the same
    /// link again, as an answer moves the generation, asks nothing more.
    pub fn take_entered(&mut self) -> Option<Gone> {
        self.entered.take()
    }

    /// Whether the pointer is over the text, so a frame can change what
    /// the preview shows.
    pub fn over_text(&self) -> bool {
        self.at.is_some()
    }

    /// What the next frame draws, or none.
    pub fn look(&self) -> Option<&LinkLook> {
        self.look.as_ref()
    }

    /// **The pointer's shape over the text**: the pointing hand while the
    /// preview shows a link a click opens -- the platform's link pointer,
    /// which each driver shows for xterm's "hand2" -- an arrow while it shows
    /// one a click would not, since nothing about it may look openable, and
    /// otherwise the program's `OSC 22` shape, `asked`, or none for the
    /// driver's own. The look with no modifier asks nothing of it: a click
    /// there selects. Over the scrollbar's strip while it takes a left press,
    /// a click scrolls, so the pointer keeps its shape there.
    pub fn pointer(&self, asked: Option<PointerShape>) -> Option<PointerShape> {
        match (&self.look, self.opens, self.left) {
            (Some(_), Opens::Yes, LeftPress::Cells) => Some(PointerShape::Hand2),
            (Some(_), Opens::Refused, LeftPress::Cells) => Some(PointerShape::Arrow),
            _ => asked,
        }
    }
}

/// **The cells of `spans`, struck, a run a rendition**: as the scan strikes
/// them at rest (`lined::push_struck`), none on a concealed cell, one a
/// program strikes itself or a double-size line.
fn struck_runs(
    term: &Term<'_>,
    spot: Spot,
    spans: &[LinkSpan],
    out: &mut Vec<baud_render::LinkLine>,
) {
    let view = term.view(spot.back);
    for sp in spans {
        let line = spot.scrolled.line(spot.back, sp.row);
        let styles = row_styles_of(term, spot.scrolled, line);
        let row = view.row(sp.row);
        let size = view.rendition(sp.row);
        for x in sp.from.index()..=sp.to.index() {
            let Some(&cell) = row.get(x) else { continue };
            if cell.content.is_wide_tail() {
                continue;
            }
            let _ = crate::lined::push_struck(
                out,
                term,
                styles,
                (sp.row, Col::at_index(x), cell),
                size,
            );
        }
    }
}

/// **Whether a link that opens is drawn as one, whole.** A URI that may have
/// been cut ([`may_be_cut`]) is not, and neither is a link on a double-size
/// line, whose cells no overlay of a single cell's width lies on.
fn drawn_whole(term: &Term<'_>, spot: Spot, link: &Under) -> bool {
    if term.view(spot.back).rendition(spot.row).is_double() {
        return false;
    }
    match link {
        Under::Osc8(_) => true,
        Under::Bare(b) => !may_be_cut(term, spot.scrolled, b.to),
        Under::Path(p) => !may_be_cut(term, spot.scrolled, p.to),
    }
}

/// The spot a click on screen cell `row`, `col` would read now: on the
/// view shown, the character column of a double-width line, none past a
/// grid that shrank under the pointer.
fn spot_now(term: &Term<'_>, scrolled: Scrolled, row: Row, col: Col) -> Option<Spot> {
    let back = term.window_back();
    let view = term.view(back);
    if row.index() >= view.rows().index() {
        return None;
    }
    Some(Spot {
        scrolled,
        back,
        row,
        col: baud_render::char_col(&view, row, col),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Host<'static> = Host("");

    fn ok(uri: &str) -> Target<'_> {
        admit(uri.as_bytes(), Host("thishost.local")).unwrap_or_else(|r| panic!("{uri}: {r:?}"))
    }

    fn no(uri: &str) -> Refused {
        admit(uri.as_bytes(), Host("thishost.local")).expect_err(uri)
    }

    /// Perturbation: add `ftp` to the allowlist -- the ftp case fails here,
    /// and `the_refusal_names_the_first_rule_that_fails` goes with it because
    /// `ftp` is the refused scheme it uses; nothing else moves.
    #[test]
    fn the_four_schemes_are_admitted_and_nothing_else_is() {
        assert_eq!(ok("http://x").scheme(), Scheme::Http);
        assert_eq!(ok("HTTPS://x").scheme(), Scheme::Https);
        assert_eq!(ok("mailto:a@b").scheme(), Scheme::Mailto);
        assert_eq!(ok("file:///tmp/x").scheme(), Scheme::File);
        for uri in [
            "ftp://x",
            "ssh://x",
            "javascript:alert(1)",
            "x-apple.systempreferences:com.apple.preference",
            "-https://x",
            "https",
            "data:text/html,hi",
        ] {
            assert_eq!(no(uri), Refused::Scheme, "{uri}");
        }
    }

    /// **The opener is handed exactly the URI the policy admitted**: the
    /// target carries the bytes it was given, not a rebuilt or trimmed form.
    /// Perturbation: make `as_str` answer anything else -- fails alone.
    #[test]
    fn the_admitted_target_is_the_uri_it_was_given() {
        for uri in [
            "https://h/p?q#f",
            "mailto:a@b?subject=s",
            "file:///Applications/Foo.app",
        ] {
            assert_eq!(ok(uri).as_str(), uri);
        }
    }

    /// Only `file` reveals; the three web-ish schemes open. Perturbation:
    /// make `action` answer `Open` for `File` -- fails alone.
    #[test]
    fn a_file_link_is_revealed_and_the_others_are_opened() {
        assert_eq!(ok("file:///Applications/Foo.app").action(), Action::Reveal);
        assert_eq!(ok("https://x").action(), Action::Open);
        assert_eq!(ok("mailto:a@b").action(), Action::Open);
    }

    /// `ls --hyperlink` names the host; only this host, `localhost` and no
    /// host are local, and the path must be absolute. Perturbation: drop the
    /// host test -- the `other` case fails alone.
    #[test]
    fn a_file_link_is_local_or_refused() {
        assert!(admit(b"file:///p", NONE).is_ok());
        assert!(admit(b"file://localhost/p", NONE).is_ok());
        assert!(admit(b"file://THISHOST.local/p", Host("thishost.local")).is_ok());
        assert_eq!(
            admit(b"file://other/p", Host("thishost.local")),
            Err(Refused::Host)
        );
        assert_eq!(admit(b"file://thishost.local/p", NONE), Err(Refused::Host));
        assert_eq!(no("file://p"), Refused::Host);
        assert_eq!(no("file:/p"), Refused::Host);
    }

    /// Perturbation: delete the authority check -- fails alone.
    #[test]
    fn a_hostless_web_url_is_refused() {
        for uri in ["http:///etc/passwd", "https://", "http://?q", "http:h"] {
            assert_eq!(no(uri), Refused::Host, "{uri}");
        }
        ok("http://h");
        ok("https://h/p?q#f");
    }

    /// Perturbation: admit every header -- the `bcc` cases fail alone.
    #[test]
    fn a_mailto_header_other_than_subject_or_body_is_refused() {
        ok("mailto:a@b");
        ok("mailto:a@b?subject=x&body=y");
        assert_eq!(no("mailto:a@b?bcc=c@d"), Refused::Header);
        assert_eq!(no("mailto:a@b?subject=x&Bcc=c"), Refused::Header);
        assert_eq!(no("mailto:a@b?attach=/etc/passwd"), Refused::Header);
        assert_eq!(no("mailto:"), Refused::Host);
    }

    /// Perturbation: widen the range to `0x20..=0x7f` -- the space and DEL
    /// cases fail, the tab, CR and high-byte cases stay green.
    #[test]
    fn a_byte_outside_printable_ascii_refuses_the_whole_uri() {
        for uri in [
            &b"https://h/a b"[..],
            b"https://h/a\tb",
            b"https://h/a\rb",
            b"https://h/a\x7fb",
            b"https://h/\xc3\xa9",
        ] {
            assert_eq!(admit(uri, NONE), Err(Refused::Byte), "{uri:?}");
        }
    }

    /// The cap is the core's, so a URI the core stored whole is never
    /// refused here for length; the control pins the two together.
    /// Perturbation: `MAX - 1` -- the 256-byte case fails.
    #[test]
    fn a_uri_at_the_cap_is_admitted_and_past_it_is_refused() {
        assert_eq!(MAX, LINK_MAX);
        let base = "https://h/";
        let at = format!("{base}{}", "a".repeat(MAX - base.len()));
        assert_eq!(at.len(), MAX);
        ok(&at);
        assert_eq!(no(&format!("{at}a")), Refused::Length);
        assert_eq!(admit(b"", NONE), Err(Refused::Length));
    }

    /// Perturbation: reorder the checks -- the reordered pair fails.
    #[test]
    fn the_refusal_names_the_first_rule_that_fails() {
        let long = format!("ftp://h/a b{}", "c".repeat(300));
        assert_eq!(no(&long), Refused::Length);
        assert_eq!(no("ftp://h/a b"), Refused::Byte);
        assert_eq!(no("ftp://h/ab"), Refused::Scheme);
    }

    #[allow(non_snake_case)]
    fn L() -> Under {
        Under::Osc8(Osc8 {
            id: LinkId::from_index(3),
            epoch: LinkEpoch::ZERO,
        })
    }

    #[allow(non_snake_case)]
    fn M() -> Under {
        Under::Osc8(Osc8 {
            id: LinkId::from_index(4),
            epoch: LinkEpoch::ZERO,
        })
    }

    fn armed(clicks: Clicks) -> Option<Armed> {
        Some(Armed { link: L(), clicks })
    }

    /// **An armed click holds through a drag and through an event that is
    /// neither a press nor a release**, so a pointer moving inside the link
    /// or a stray event between the down and the up does not lose the click.
    /// Perturbation: drop on `Press::Other` -- fails alone.
    #[test]
    fn an_armed_click_holds_through_a_drag_and_any_other_event() {
        for press in [Press::Dragged, Press::Other] {
            let g = gesture(
                press,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                armed(Clicks::Single).as_ref(),
                Some(&L()),
            );
            assert_eq!(g, Gesture::Hold, "{press:?}");
        }
    }

    /// Perturbation: require `Chord::Cmd` on the Up -- the second case
    /// fails alone, and users who release Cmd before the button would have
    /// lost the click.
    #[test]
    fn a_cmd_click_on_a_link_arms_on_the_down_and_opens_on_the_up() {
        let down = gesture(
            Press::Down,
            Chord::Cmd,
            Clicks::Single,
            Focus::Gained,
            None,
            Some(&L()),
        );
        assert_eq!(
            down,
            Gesture::Arm(Armed {
                link: L(),
                clicks: Clicks::Single
            })
        );
        let up = gesture(
            Press::Up,
            Chord::Other,
            Clicks::Single,
            Focus::Gained,
            armed(Clicks::Single).as_ref(),
            Some(&L()),
        );
        assert_eq!(up, Gesture::Open(L()));
    }

    /// The release opens over the *same link*, not the same cell: a drift
    /// across a cell boundary inside the link, or lead to tail, still
    /// opens; a release over another link or off one drops. Perturbation:
    /// open on any release while armed -- the `M` and `None` cases fail.
    #[test]
    fn a_press_that_leaves_the_link_opens_nothing_and_still_swallows_the_up() {
        let a = armed(Clicks::Single);
        let a = a.as_ref();
        assert_eq!(
            gesture(
                Press::Dragged,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                a,
                Some(&L())
            ),
            Gesture::Hold
        );
        assert_eq!(
            gesture(
                Press::Up,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                a,
                Some(&M())
            ),
            Gesture::Drop
        );
        assert_eq!(
            gesture(
                Press::Up,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                a,
                None
            ),
            Gesture::Drop
        );
        assert_eq!(
            gesture(
                Press::Exited,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                a,
                Some(&L())
            ),
            Gesture::Drop
        );
    }

    /// Perturbation: read `Chord::Other` as Cmd -- fails alone.
    #[test]
    fn shift_beats_cmd_so_a_cmd_shift_click_is_a_selection() {
        assert_eq!(
            gesture(
                Press::Down,
                Chord::Other,
                Clicks::Single,
                Focus::Gained,
                None,
                Some(&L())
            ),
            Gesture::Pass
        );
    }

    /// Perturbation: arm on any Cmd press -- fails alone.
    #[test]
    fn a_cmd_click_off_a_link_is_not_swallowed() {
        assert_eq!(
            gesture(
                Press::Down,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                None,
                None
            ),
            Gesture::Pass
        );
    }

    /// A Terminal.app user Cmd-double-clicks: the second press must not
    /// leave a word selection behind or reach a tracking child, and must
    /// not open twice. Perturbation: pass a repeat press -- fails alone.
    #[test]
    fn a_repeat_click_on_a_link_is_swallowed_whole_and_opens_nothing() {
        let down = gesture(
            Press::Down,
            Chord::Cmd,
            Clicks::Repeat,
            Focus::Gained,
            None,
            Some(&L()),
        );
        assert_eq!(
            down,
            Gesture::Arm(Armed {
                link: L(),
                clicks: Clicks::Repeat
            })
        );
        let up = gesture(
            Press::Up,
            Chord::Cmd,
            Clicks::Repeat,
            Focus::Gained,
            armed(Clicks::Repeat).as_ref(),
            Some(&L()),
        );
        assert_eq!(up, Gesture::Drop);
    }

    /// Perturbation: drop the focus test -- fails alone.
    #[test]
    fn a_press_into_a_background_window_arms_nothing() {
        assert_eq!(
            gesture(
                Press::Down,
                Chord::Cmd,
                Clicks::Single,
                Focus::Lost,
                None,
                Some(&L())
            ),
            Gesture::Pass
        );
    }

    // --- A URI in the text -------------------------------------------

    /// A window over `s` with the click on the character after `‸`; a
    /// combining acute after a character marks that cell.
    fn win(s: &str) -> Window {
        let mut text = Vec::new();
        let mut at = None;
        for ch in s.chars() {
            match ch {
                '‸' => at = Some(text.len()),
                '\u{301}' => {
                    if let Some(last) = text.last_mut() {
                        *last = Unit::Marked;
                    }
                }
                c => text.push(Unit::Char(c)),
            }
        }
        let cells = (0..text.len())
            .map(|i| Point::new(AbsLine::at(Rows::ZERO), Col::at_index(i)))
            .collect();
        Window {
            text,
            cells,
            at: Ix(at.expect("a ‸ in the text")),
            head: Edge::Line,
            tail: Edge::Line,
        }
    }

    /// The URI `find` reads around the click, or why it is refused.
    fn read_in(w: &Window) -> Option<Result<String, Refused>> {
        find(w).map(|f| {
            f.verdict.map(|()| {
                w.text[f.span]
                    .iter()
                    .map(|u| match u {
                        Unit::Char(c) => *c,
                        Unit::Marked | Unit::Gap => '?',
                    })
                    .collect()
            })
        })
    }

    fn read(s: &str) -> Option<Result<String, Refused>> {
        read_in(&win(s))
    }

    fn got(s: &str) -> String {
        match read(s) {
            Some(Ok(uri)) => uri,
            other => panic!("{s}: {other:?}"),
        }
    }

    /// RFC 3986 Appendix C's own example: the double quotes and the angle
    /// brackets are wrappers, not part of the URI, and the `ftp` one is
    /// not a scheme `admit` lets through. Perturbation: put `"` in the
    /// URI class -- the first case fails alone.
    #[test]
    fn the_rfc_s_wrappers_are_not_part_of_the_uri() {
        assert_eq!(
            got(r#"Yes, Jim, I found it under "http://www.‸w3.org/Addressing/","#),
            "http://www.w3.org/Addressing/"
        );
        assert_eq!(
            got(
                "Note the warning in <http://www.ics.uci.edu/pub/ietf/uri/historical.html#WA‸RNING>."
            ),
            "http://www.ics.uci.edu/pub/ietf/uri/historical.html#WARNING"
        );
        assert_eq!(read("from <ftp://foo.example.com/r‸fc/>."), None);
    }

    /// Prose punctuation after a URI is the sentence's; inside it, the
    /// URI's. A click on the trimmed character is not on the URI.
    /// Perturbation: empty `TRAILING` -- every trimmed case fails, and
    /// three tests with a case ending in `.` with them.
    #[test]
    fn trailing_punctuation_is_the_sentence_s() {
        assert_eq!(got("Visit https://example.‸com."), "https://example.com");
        assert_eq!(
            got("Go to https://example.com/pa‸th/, then"),
            "https://example.com/path/"
        );
        assert_eq!(
            got("is it https://exa‸mple.com/a?b=c?!"),
            "https://example.com/a?b=c"
        );
        assert_eq!(
            got("'https://example.com/it‸'s'"),
            "https://example.com/it's"
        );
        assert_eq!(got("**https://exa‸mple.com**"), "https://example.com");
        assert_eq!(
            got("https://example.com/a.b,c;d‸=e"),
            "https://example.com/a.b,c;d=e"
        );
        assert_eq!(read("Visit https://example.com‸."), None);
    }

    /// Wikipedia's parentheses stay; one that closes the sentence's goes;
    /// an opening bracket before the scheme ends the URI at its closer, so
    /// both halves of a Markdown link open the address. Perturbation:
    /// trim every trailing `)` -- the Wikipedia cases fail; drop the
    /// wrapper rule -- the `[https://...]` case is refused instead.
    #[test]
    fn parentheses_are_balanced_and_a_wrapper_ends_the_uri() {
        assert_eq!(
            got("see https://en.wikipedia.org/wiki/Rust_(pro‸gramming_language)."),
            "https://en.wikipedia.org/wiki/Rust_(programming_language)"
        );
        assert_eq!(
            got("(see https://exa‸mple.com/a)."),
            "https://example.com/a"
        );
        assert_eq!(
            got("(https://en.wikipedia.org/wiki/Rust_(l‸ang))"),
            "https://en.wikipedia.org/wiki/Rust_(lang)"
        );
        assert_eq!(
            got("[docs](https://example.com/do‸cs)"),
            "https://example.com/docs"
        );
        assert_eq!(
            got("[https://exam‸ple.com](https://example.com)"),
            "https://example.com"
        );
        assert_eq!(
            got("[https://example.com](https://exa‸mple.com)"),
            "https://example.com"
        );
    }

    /// Brackets stand around an IP-literal host and nowhere else
    /// (§ 3.2.2), and a URI that breaks the rule is refused whole rather
    /// than opened cut short. Perturbation: skip the check past the
    /// authority -- the path case is admitted.
    #[test]
    fn brackets_stand_only_around_an_ip_literal_host() {
        assert_eq!(
            got("http://[::1]:80‸80/status."),
            "http://[::1]:8080/status"
        );
        assert_eq!(got("http://[::‸1]/"), "http://[::1]/");
        assert_eq!(
            read("https://example.com/a[1‸]"),
            Some(Err(Refused::Syntax))
        );
        assert_eq!(read("https://ex[am]ple.co‸m/"), Some(Err(Refused::Syntax)));
        assert_eq!(read("https://[::1]x‸/"), Some(Err(Refused::Syntax)));
    }

    /// `%` begins `pct-encoded` (§ 2.1) or the URI is refused. Perturbation:
    /// skip the check -- the two refused cases are admitted.
    #[test]
    fn a_percent_without_two_hex_digits_refuses_the_uri() {
        assert_eq!(
            got("https://example.com/a%2‸0b"),
            "https://example.com/a%20b"
        );
        assert_eq!(
            read("https://example.com/a%2‸g"),
            Some(Err(Refused::Syntax))
        );
        assert_eq!(
            read("https://example.com/10‸0%"),
            Some(Err(Refused::Syntax))
        );
    }

    /// A scheme starts where one may (§ 3.1) and is one `admit` lets
    /// through, matched without case. Perturbation: drop the boundary test
    /// -- `xhttps` reads as `https`.
    #[test]
    fn the_uri_starts_at_an_admitted_scheme_and_nowhere_else() {
        assert_eq!(read("xhttps://exam‸ple.com"), None);
        assert_eq!(read("ftp://exam‸ple.com"), None);
        assert_eq!(read("https:‸//"), None);
        assert_eq!(read("example.co‸m"), None);
        assert_eq!(got("HTTPS://Exam‸ple.com/"), "HTTPS://Example.com/");
        assert_eq!(got("url=https://exam‸ple.com"), "https://example.com");
        assert_eq!(
            got("mail mailto:someone@exa‸mple.com."),
            "mailto:someone@example.com"
        );
        assert_eq!(got("file:///tmp/r‸eport.txt"), "file:///tmp/report.txt");
        assert_eq!(
            got("https://a.example/?u=https://b.exa‸mple/"),
            "https://a.example/?u=https://b.example/"
        );
    }

    /// A wide or accented letter running on from a URI makes it one the
    /// text names and `admit` cannot open as shown: refused, not cut. Wide
    /// text before the scheme, or CJK punctuation after it, does not.
    /// Perturbation: drop the run-on test -- the refused cases open cut.
    #[test]
    fn a_letter_outside_ascii_running_on_refuses_the_uri() {
        assert_eq!(
            read("見https://exam‸ple.com/a見"),
            Some(Err(Refused::Syntax))
        );
        assert_eq!(
            read("https://example.com/ca‸fé"),
            Some(Err(Refused::Syntax))
        );
        assert_eq!(
            read("https://example.com/ca‸fe\u{301}"),
            Some(Err(Refused::Syntax))
        );
        assert_eq!(got("見https://exam‸ple.com/a 見"), "https://example.com/a");
        assert_eq!(got("https://exam‸ple.com/a。"), "https://example.com/a");
    }

    /// A click on a cell with no URI around it reads nothing.
    #[test]
    fn a_cell_with_no_uri_reads_nothing() {
        assert_eq!(read("just some w‸ords here"), None);
        assert_eq!(read("before‸ https://example.com"), None);
        assert_eq!(read("https://example.com‸ after"), None);
        assert_eq!(read("‸見https://example.com"), None);
    }

    /// A window the walk stopped short of the line on refuses a URI that
    /// reaches the cut, since what lies past it may be the URI's; the same
    /// text with the line's own end opens. Perturbation: ignore the edges
    /// -- the cut cases open.
    #[test]
    fn a_uri_that_reaches_a_cut_is_refused_for_length() {
        let s = "https://exam‸ple.com/abc";
        let mut w = win(s);
        assert_eq!(read_in(&w), Some(Ok("https://example.com/abc".to_string())));
        w.tail = Edge::Cut;
        assert_eq!(read_in(&w), Some(Err(Refused::Length)));
        let mut w = win(s);
        w.head = Edge::Cut;
        assert_eq!(read_in(&w), Some(Err(Refused::Length)));
    }

    // --- The click, over a real grid -----------------------------------

    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 6,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    fn spot(scrolled: Scrolled, row: Row, col: Col) -> Spot {
        Spot {
            scrolled,
            back: Rows::ZERO,
            row,
            col,
        }
    }

    /// The press and release the driver would make of a Cmd-click at
    /// `down` and `up`: what the release does.
    fn click(t: &Term<'_>, down: Spot, up: Spot) -> Gesture {
        let under_down = under(t, down, Press::Down, Chord::Cmd, Focus::Gained, None);
        let armed = match gesture(
            Press::Down,
            Chord::Cmd,
            Clicks::Single,
            Focus::Gained,
            None,
            under_down.as_ref(),
        ) {
            Gesture::Arm(a) => a,
            other => return other,
        };
        let under_up = under(t, up, Press::Up, Chord::Other, Focus::Gained, Some(&armed));
        gesture(
            Press::Up,
            Chord::Other,
            Clicks::Single,
            Focus::Gained,
            Some(&armed),
            under_up.as_ref(),
        )
    }

    fn opened(g: &Gesture) -> Option<Vec<u8>> {
        match g {
            Gesture::Open(Under::Bare(b)) => b.uri().ok().map(<[u8]>::to_vec),
            _ => None,
        }
    }

    /// **The whole click path over a URL the writer soft-wrapped across
    /// three rows of a 20-column grid**: armed on the press in its middle
    /// row, opened on the release in its last, and what opens is the whole
    /// URL, admitted by the same policy as an `OSC 8` link. On the base
    /// the press passed, and the click reached the selection.
    /// Perturbation: stop the walk at the row's end -- the URL is cut at
    /// the wrap and the bytes differ.
    #[test]
    fn a_cmd_click_on_a_soft_wrapped_url_opens_the_whole_url() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let url = "https://example.com/a/very/long/path/to/it";
        feed(&mut t, format!("see {url} ok").as_bytes());
        let s = Scrolled::START;
        let g = click(
            &t,
            spot(s, Row::from_index(1), Col::at_index(5)),
            spot(s, Row::from_index(2), Col::at_index(1)),
        );
        assert_eq!(opened(&g).as_deref(), Some(url.as_bytes()), "{g:?}");
        let Gesture::Open(Under::Bare(b)) = g else {
            unreachable!()
        };
        let target = admit(b.uri().unwrap(), Host("")).expect("admitted");
        assert_eq!(target.as_str(), url);
        assert_eq!(target.action(), Action::Open);
    }

    /// The shape of RFC 3986 Appendix C's example, hard-broken inside its
    /// `<`: the text names a longer address than the row holds, so the
    /// click refuses it rather than open the prefix; the same text on one
    /// line opens the whole URI, and a closed wrapper opens as before.
    /// Perturbation: drop the wrapper test -- the prefix opens.
    #[test]
    fn an_unclosed_wrapper_at_a_hard_break_is_refused_not_opened_short() {
        let s = Scrolled::START;
        let at = spot(s, Row::from_index(0), Col::at_index(8));
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"<http://h.ex/pub/\r\nietf/x>.");
        match click(&t, at, at) {
            Gesture::Open(Under::Bare(b)) => assert_eq!(b.uri(), Err(Refused::Length)),
            other => panic!("{other:?}"),
        }
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"<http://h.ex/pub/ietf/x>.");
        assert_eq!(
            opened(&click(&t, at, at)).as_deref(),
            Some(&b"http://h.ex/pub/ietf/x"[..])
        );
        assert_eq!(read("<‸http://h/a>"), Some(Ok("http://h/a".into())));
        assert_eq!(read("\"‸http://h/a"), Some(Err(Refused::Length)));
    }

    /// A hard line break ends the URI: the next row is not joined.
    /// Perturbation: join every row -- `bcd` is read on.
    #[test]
    fn a_hard_line_break_ends_the_uri() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"https://example.co/a\r\nbcd");
        let s = Scrolled::START;
        let g = click(
            &t,
            spot(s, Row::from_index(0), Col::at_index(9)),
            spot(s, Row::from_index(0), Col::at_index(9)),
        );
        assert_eq!(opened(&g).as_deref(), Some(&b"https://example.co/a"[..]));
    }

    /// A URL whose first rows have scrolled into the history is read whole
    /// from its last row on the live screen; and output that scrolls it
    /// between the press and the release keeps it the same link, because
    /// its cells are named by absolute line. Perturbation: stop the walk
    /// at the row above -- the first case is cut.
    #[test]
    fn a_url_reaching_into_the_history_or_scrolled_under_the_press_opens() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let url = "https://example.com/a/very/long/path";
        feed(&mut t, format!("{url}\r\n1\r\n2\r\n3\r\n4\r\n5").as_bytes());
        // The URL's first row went into the history; its last is row 0.
        let s = Scrolled::START.advanced(Rows::of_count(1));
        let g = click(
            &t,
            spot(s, Row::from_index(0), Col::at_index(1)),
            spot(s, Row::from_index(0), Col::at_index(2)),
        );
        assert_eq!(opened(&g).as_deref(), Some(url.as_bytes()), "{g:?}");

        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"a\r\nb\r\nhttps://ex.com/x\r\nc");
        let s = Scrolled::START;
        let down = under(
            &t,
            spot(s, Row::from_index(2), Col::at_index(9)),
            Press::Down,
            Chord::Cmd,
            Focus::Gained,
            None,
        );
        let Some(Gesture::Arm(armed)) = Some(gesture(
            Press::Down,
            Chord::Cmd,
            Clicks::Single,
            Focus::Gained,
            None,
            down.as_ref(),
        )) else {
            unreachable!()
        };
        feed(&mut t, b"\r\nd\r\ne\r\nf");
        // One line retired: the URL's row is row 1 now.
        let s = s.advanced(Rows::of_count(1));
        let up = under(
            &t,
            spot(s, Row::from_index(1), Col::at_index(3)),
            Press::Up,
            Chord::Other,
            Focus::Gained,
            Some(&armed),
        );
        let g = gesture(
            Press::Up,
            Chord::Other,
            Clicks::Single,
            Focus::Gained,
            Some(&armed),
            up.as_ref(),
        );
        assert_eq!(
            opened(&g).as_deref(),
            Some(&b"https://ex.com/x"[..]),
            "{g:?}"
        );
    }

    /// Text rewritten under a held press is not the link that was armed:
    /// the release drops it. Perturbation: compare the cells and not the
    /// bytes -- the rewritten URL opens.
    #[test]
    fn a_url_rewritten_under_the_press_opens_nothing() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"https://ex.com/aaaa");
        let s = Scrolled::START;
        let down = under(
            &t,
            spot(s, Row::from_index(0), Col::at_index(9)),
            Press::Down,
            Chord::Cmd,
            Focus::Gained,
            None,
        );
        let Gesture::Arm(armed) = gesture(
            Press::Down,
            Chord::Cmd,
            Clicks::Single,
            Focus::Gained,
            None,
            down.as_ref(),
        ) else {
            panic!("armed")
        };
        feed(&mut t, b"\rhttps://ev.com/bbbb");
        let up = under(
            &t,
            spot(s, Row::from_index(0), Col::at_index(9)),
            Press::Up,
            Chord::Other,
            Focus::Gained,
            Some(&armed),
        );
        let g = gesture(
            Press::Up,
            Chord::Other,
            Clicks::Single,
            Focus::Gained,
            Some(&armed),
            up.as_ref(),
        );
        assert_eq!(g, Gesture::Drop);
    }

    /// **A press on a link whose slot a sweep gave to another target opens
    /// nothing.** The core frees a link table slot once nothing names it,
    /// and the next target may take it; here the pressed link's cell is
    /// erased while the press is held and another link lands on the same
    /// cell under the same index, so the release is over a cell naming the
    /// pressed index -- and another target. The epoch the press was read
    /// under has moved, so it is not the link that was pressed.
    /// Perturbation: read every link under the first epoch -- the release
    /// opens the other target.
    #[test]
    fn a_press_on_a_link_whose_slot_was_given_away_opens_nothing() {
        let link = |uri: &str, text: &str| format!("\x1b]8;;{uri}\x1b\\{text}\x1b]8;;\x1b\\");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, link("https://pressed.example/", "A").as_bytes());
        let at = spot(Scrolled::START, Row::from_index(0), Col::at_index(0));
        let down = under(&t, at, Press::Down, Chord::Cmd, Focus::Gained, None);
        let Some(Under::Osc8(pressed)) = down.clone() else {
            panic!("the press is over the link: {down:?}")
        };
        let Gesture::Arm(armed) = gesture(
            Press::Down,
            Chord::Cmd,
            Clicks::Single,
            Focus::Gained,
            None,
            down.as_ref(),
        ) else {
            panic!("armed")
        };
        // While the press is held: fill the table, erase everything, write
        // a screen of text, and link the pressed cell again and again until
        // its new target took the pressed one's slot.
        for i in 0..254 {
            feed(
                &mut t,
                link(&format!("https://fill.example/{i}"), "f").as_bytes(),
            );
        }
        feed(&mut t, b"\x1b[H\x1b[2J\x1b[3J");
        feed(&mut t, &[b'.'; 20 * 6]);
        feed(&mut t, b"\x1b[H\x1b[2J");
        let mut took = false;
        for i in 0..400 {
            let next = format!("\x1b[H{}", link(&format!("https://other.example/{i}"), "B"));
            feed(&mut t, next.as_bytes());
            let cell = t.view(Rows::ZERO).row(Row::from_index(0))[0];
            if cell.flags.link() == pressed.id {
                took = true;
                break;
            }
        }
        assert!(took, "the control: another target took the pressed slot");
        let up = under(&t, at, Press::Up, Chord::Other, Focus::Gained, Some(&armed));
        let uri = match &up {
            Some(Under::Osc8(l)) => l.uri(&t),
            _ => None,
        };
        assert!(
            uri.is_some_and(|u| u.starts_with(b"https://other.example/")),
            "the control: the release is over the other target, {up:?}"
        );
        let g = gesture(
            Press::Up,
            Chord::Other,
            Clicks::Single,
            Focus::Gained,
            Some(&armed),
            up.as_ref(),
        );
        assert_eq!(g, Gesture::Drop);
        assert_eq!(pressed.uri(&t), None, "and the pressed link is gone");
    }

    /// An `OSC 8` cell is the `OSC 8` link's, and it ends the text around
    /// it; so does a concealed cell, so what opens is what is seen.
    /// Perturbation: read a linked or concealed cell as its character --
    /// the text runs on into it.
    #[test]
    fn an_osc_8_or_concealed_cell_ends_a_uri_in_the_text() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            b"https://a.example/\x1b]8;;https://osc.example/\x1b\\x\x1b]8;;\x1b\\\r\nhttps://b.ex/\x1b[8mhid\x1b[0m",
        );
        let s = Scrolled::START;
        let osc = under(
            &t,
            spot(s, Row::from_index(0), Col::at_index(18)),
            Press::Down,
            Chord::Cmd,
            Focus::Gained,
            None,
        );
        assert!(matches!(osc, Some(Under::Osc8(_))), "{osc:?}");
        let g = click(
            &t,
            spot(s, Row::from_index(0), Col::at_index(9)),
            spot(s, Row::from_index(0), Col::at_index(9)),
        );
        assert_eq!(
            opened(&g).as_deref(),
            Some(&b"https://a.example/"[..]),
            "{g:?}"
        );
        let g = click(
            &t,
            spot(s, Row::from_index(1), Col::at_index(9)),
            spot(s, Row::from_index(1), Col::at_index(9)),
        );
        assert_eq!(opened(&g).as_deref(), Some(&b"https://b.ex/"[..]), "{g:?}");
    }

    /// A wide character is a letter outside ASCII: before the scheme it
    /// is the sentence's, running on from the URI it refuses it, and a
    /// click on its tail is a click on it, which reads nothing.
    #[test]
    fn a_wide_character_beside_a_url_is_read_as_a_letter() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, "見https://e.co/a 見\r\nhttps://e.co/b見".as_bytes());
        let s = Scrolled::START;
        let g = click(
            &t,
            spot(s, Row::from_index(0), Col::at_index(6)),
            spot(s, Row::from_index(0), Col::at_index(6)),
        );
        assert_eq!(opened(&g).as_deref(), Some(&b"https://e.co/a"[..]), "{g:?}");
        let g = click(
            &t,
            spot(s, Row::from_index(1), Col::at_index(6)),
            spot(s, Row::from_index(1), Col::at_index(6)),
        );
        match g {
            Gesture::Open(Under::Bare(b)) => {
                assert_eq!(b.uri(), Err(Refused::Syntax));
                assert_eq!(b.text(), b"https://e.co/b", "what the refusal logs");
            }
            other => panic!("{other:?}"),
        }
        let tail = under(
            &t,
            spot(s, Row::from_index(1), Col::at_index(15)),
            Press::Down,
            Chord::Cmd,
            Focus::Gained,
            None,
        );
        assert_eq!(tail, None);
    }

    /// **The text is read on an unarmed Cmd press in a key window and on
    /// the release of a press armed over text, and on nothing else** --
    /// a move, a drag, a press without Cmd, a press into a background
    /// window, an unarmed release -- so nothing a frame or a pointer
    /// motion does reads it. Perturbation: read on every event -- the
    /// drag and the plain press read a URI.
    #[test]
    fn the_text_is_read_only_for_the_press_and_its_release() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"https://example.com/a");
        let at = spot(Scrolled::START, Row::ZERO, Col::at_index(9));
        let read = |press, chord, focus, armed| under(&t, at, press, chord, focus, armed);
        let armed = match read(Press::Down, Chord::Cmd, Focus::Gained, None) {
            Some(link @ Under::Bare(_)) => Armed {
                link,
                clicks: Clicks::Single,
            },
            other => panic!("{other:?}"),
        };
        assert!(read(Press::Up, Chord::Other, Focus::Gained, Some(&armed)).is_some());
        for (press, chord, focus) in [
            (Press::Dragged, Chord::Cmd, Focus::Gained),
            (Press::Other, Chord::Cmd, Focus::Gained),
            (Press::Exited, Chord::Cmd, Focus::Gained),
            (Press::Up, Chord::Cmd, Focus::Gained),
            (Press::Down, Chord::Other, Focus::Gained),
            (Press::Down, Chord::Cmd, Focus::Lost),
        ] {
            assert_eq!(
                read(press, chord, focus, None),
                None,
                "{press:?} {chord:?} {focus:?}"
            );
        }
        assert_eq!(
            read(Press::Dragged, Chord::Cmd, Focus::Gained, Some(&armed)),
            None
        );
    }

    // --- Seeing where a link goes ---------------------------------------

    /// **The preview shows every byte a target holds, and nothing that
    /// could pass for another address**: controls, a bidi override and an
    /// isolate, a zero-width space, a look-alike letter of another script,
    /// a space and DEL each as `\xNN`, a backslash doubled, so what is
    /// drawn is printable ASCII and one line. The control: an address that
    /// opens is shown as it is. Perturbation: show bytes from `0x80` as
    /// they decode -- the look-alike's line fails.
    #[test]
    fn the_preview_shows_every_byte_a_target_holds() {
        assert_eq!(
            escaped(b"https://example.com/a?b=c#d"),
            "https://example.com/a?b=c#d",
            "the control"
        );
        for (uri, want) in [
            (&b"https://x/\x1b[31m\x07"[..], "https://x/\\x1B[31m\\x07"),
            (
                "https://x/\u{202e}fdp.exe".as_bytes(),
                "https://x/\\xE2\\x80\\xAEfdp.exe",
            ),
            (
                "https://x/\u{2066}a".as_bytes(),
                "https://x/\\xE2\\x81\\xA6a",
            ),
            (
                "https://x/a\u{200b}b".as_bytes(),
                "https://x/a\\xE2\\x80\\x8Bb",
            ),
            (
                "https://\u{430}pple.example/".as_bytes(),
                "https://\\xD0\\xB0pple.example/",
            ),
            (b"https://x/a b\x7f", "https://x/a\\x20b\\x7F"),
            (b"https://x/a\\b", "https://x/a\\\\b"),
            (b"https://good.com@evil.com/", "https://good.com@evil.com/"),
        ] {
            let shown = escaped(uri);
            assert_eq!(shown, want, "{uri:?}");
            assert!(shown.bytes().all(|b| (0x21..=0x7e).contains(&b)), "{shown}");
        }
    }

    /// **A long address keeps its scheme and the end of its host**, where
    /// the registrable domain is, and the end of what follows: a host that
    /// starts like another site's shows where it goes. Every room from 3
    /// columns to past the address fits, and an address that fits is
    /// whole. Perturbation: cut from the end alone -- `evil.test` is lost.
    #[test]
    fn a_long_address_keeps_its_scheme_and_the_end_of_its_host() {
        let host = format!("accounts.example.com.{}.evil.test", "a".repeat(40));
        let long = format!("https://{host}/login?next=/settings/keys");
        let at = |room| shortened(&long, Cols::of_index(room));
        assert_eq!(at(long.len()), long, "the control: it fits");
        let s = at(62);
        assert!(s.starts_with("https://\u{2026}"), "{s}");
        assert!(s.contains("evil.test\u{2026}"), "{s}");
        assert!(s.ends_with("/settings/keys"), "{s}");
        // A host that fits is whole, and the rest's end follows it.
        let short = "https://example.com/a/very/long/path/to/a/file/somewhere/deep/inside.txt";
        let s = shortened(short, Cols::of_index(40));
        assert!(s.starts_with("https://example.com\u{2026}"), "{s}");
        assert!(s.ends_with("inside.txt"), "{s}");
        // `mailto` keeps the address's domain; userinfo before a host
        // keeps the host.
        let s = shortened(
            &format!("mailto:{}@evil.test", "a".repeat(60)),
            Cols::of_index(30),
        );
        assert!(
            s.starts_with("mailto:\u{2026}") && s.ends_with("@evil.test"),
            "{s}"
        );
        let s = shortened(
            &format!("https://good.com{}@evil.com/x", "a".repeat(60)),
            Cols::of_index(30),
        );
        assert!(s.contains("@evil.com"), "{s}");
        for text in [long.as_str(), short] {
            for room in 3..=text.len() + 2 {
                let n = shortened(text, Cols::of_index(room)).chars().count();
                assert!(n <= room, "{room}: {n}");
                assert!(n == room || room > text.len(), "{room}: {n} leaves room");
            }
        }
        // The longest target the core keeps, shown in a row.
        let max = format!("https://x/{}", "p".repeat(MAX - 10));
        assert_eq!(
            shortened(&escaped(max.as_bytes()), Cols::of_index(78))
                .chars()
                .count(),
            78
        );
    }

    /// **A link the click refuses says why before the click**, in words;
    /// one that opens shows its address alone; a row too narrow for the
    /// reason says only that it will not open. Perturbation: no label on a
    /// refusal -- the ftp line fails.
    #[test]
    fn a_refused_link_says_why_before_the_click() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b]8;;ftp://f.example/x\x1b\\ftp\x1b]8;;\x1b\\ \x1b]8;;https://h.example/\x1b\\web\x1b]8;;\x1b\\");
        let link = |col| hovered(&t, spot(Scrolled::START, Row::ZERO, Col::at_index(col))).unwrap();
        let text =
            |col, room| preview_text(&link(col), &t, Host(""), Cols::of_index(room)).unwrap();
        assert_eq!(
            text(0, 80),
            "Won't open, not a web, mail or file link  ftp://f.example/x"
        );
        assert_eq!(text(4, 80), "https://h.example/", "the control: it opens");
        assert_eq!(text(0, 30), "Won't open  ftp://f.example/x", "a narrow row");
    }

    /// **The bar moves off the link**: the bottom row's left end, its right
    /// end while the link is under the left, the top row while it is under
    /// both. The control: a link elsewhere leaves it bottom left.
    #[test]
    fn the_bar_moves_off_the_link() {
        let s = |r: usize, a: usize, b: usize| LinkSpan {
            row: Row::from_index(r),
            from: Col::at_index(a),
            to: Col::at_index(b),
        };
        let (last, top) = (Row::from_index(9), Row::from_index(0));
        assert_eq!(
            bar_at(
                &[s(3, 0, 5)],
                Cols::of_index(20),
                Cols::of_index(80),
                Rows::of_index(10)
            ),
            (last, Col::ZERO),
            "the control"
        );
        assert_eq!(
            bar_at(
                &[s(9, 70, 79)],
                Cols::of_index(20),
                Cols::of_index(80),
                Rows::of_index(10)
            ),
            (last, Col::ZERO)
        );
        assert_eq!(
            bar_at(
                &[s(9, 5, 9)],
                Cols::of_index(20),
                Cols::of_index(80),
                Rows::of_index(10)
            ),
            (last, Col::at_index(60))
        );
        assert_eq!(
            bar_at(
                &[s(9, 5, 70)],
                Cols::of_index(20),
                Cols::of_index(80),
                Rows::of_index(10)
            ),
            (top, Col::ZERO)
        );
    }

    /// **Every cell naming the link is underlined**, connected or not --
    /// one URI and one `id` -- and no cell of another link. The control: a
    /// second link between the two runs is not underlined.
    #[test]
    fn every_cell_naming_the_link_is_underlined() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            b"\x1b]8;id=a;https://a.example/\x1b\\ab\x1b]8;;\x1b\\ \x1b]8;;https://b.example/\x1b\\cd\x1b]8;;\x1b\\\r\n  \x1b]8;id=a;https://a.example/\x1b\\ef\x1b]8;;\x1b\\",
        );
        let at = spot(Scrolled::START, Row::ZERO, Col::ZERO);
        let link = hovered(&t, at).unwrap();
        let s = |r: usize, a: usize, b: usize| LinkSpan {
            row: Row::from_index(r),
            from: Col::at_index(a),
            to: Col::at_index(b),
        };
        assert_eq!(link_spans(&t, at, &link), vec![s(0, 0, 1), s(1, 2, 3)]);
        let other = hovered(&t, spot(Scrolled::START, Row::ZERO, Col::at_index(3))).unwrap();
        assert_eq!(link_spans(&t, at, &other), vec![s(0, 3, 4)], "the control");
    }

    // --- The preview held while the screen moves -----------------------

    /// A 60-column grid, wide enough that no address here is shortened.
    const WIDE: WinSize = WinSize {
        ws_col: 60,
        ws_row: 6,
    };

    /// Fourteen `OSC 8` links, one a line, `https://l{n}.example/`.
    fn numbered_links() -> Vec<u8> {
        let mut bytes = Vec::new();
        for n in 0..14 {
            bytes.extend_from_slice(
                format!("\x1b]8;;https://l{n}.example/\x1b\\L{n}\x1b]8;;\x1b\\\r\n").as_bytes(),
            );
        }
        bytes
    }

    /// The bar's text, as drawn.
    fn bar_string(l: &LinkLook) -> String {
        l.bar
            .text
            .iter()
            .filter_map(|c| char::from_u32(c.value()))
            .collect()
    }

    /// What a pointer event at the top-left cell would preview now: the
    /// spot a click there would read, built from the view shown.
    fn fresh(t: &Term<'_>, scrolled: Scrolled) -> Option<LinkLook> {
        let s = Spot {
            scrolled,
            back: t.window_back(),
            row: Row::ZERO,
            col: Col::ZERO,
        };
        preview(t, s, &hovered(t, s)?, NONE)
    }

    /// **A preview held over a window on the history follows the view
    /// shown**: the pointer rests on the top row of a window three lines
    /// up, and two lines of output move the window up with its line, so
    /// the view shows the same lines; the frame's refresh previews the
    /// link drawn under the pointer, as a click there would read it. The
    /// control: before the output, the preview is the fresh one.
    /// Perturbation: refresh with the `back` of the last pointer event --
    /// the bar after the output fails.
    #[test]
    fn a_held_preview_follows_the_view_as_output_moves_it() {
        let mut owned = Owned::with_history(
            &WIDE,
            &baud_core::CacheGeometry::UNKNOWN,
            baud_core::Depth::of(50),
        );
        let mut t = Term::new(owned.storage(), &WIDE);
        feed(&mut t, &numbered_links());
        t.set_window_back(Rows::of_count(3));
        let at = Some((Row::ZERO, Col::ZERO));
        let mut h = Hover::default();
        let s = Scrolled::START;
        assert!(h.update(&t, at, s, Chord::Cmd, Focus::Gained, NONE));
        assert_eq!(h.look(), fresh(&t, Scrolled::START).as_ref(), "the control");
        let shown = h.look().map(bar_string);
        feed(&mut t, b"more\r\nmore\r\n");
        assert_eq!(t.window_back(), Rows::of_count(5), "held on its line");
        let _ = h.refresh(&t, s, Chord::Cmd, Focus::Gained, NONE);
        assert_eq!(h.look().map(bar_string), shown, "the same link drawn there");
        assert_eq!(h.look(), fresh(&t, Scrolled::START).as_ref());
    }

    /// **A wheel under a still pointer with Command held**: the window
    /// moves over the history with no pointer event, and the next frame's
    /// refresh previews the link the pointer is over now. Perturbation:
    /// refresh with the `back` of the last pointer event -- this fails.
    #[test]
    fn a_wheel_under_a_held_preview_reads_the_new_view() {
        let mut owned = Owned::with_history(
            &WIDE,
            &baud_core::CacheGeometry::UNKNOWN,
            baud_core::Depth::of(50),
        );
        let mut t = Term::new(owned.storage(), &WIDE);
        feed(&mut t, &numbered_links());
        t.set_window_back(Rows::of_count(3));
        let at = Some((Row::ZERO, Col::ZERO));
        let mut h = Hover::default();
        let s = Scrolled::START;
        assert!(h.update(&t, at, s, Chord::Cmd, Focus::Gained, NONE));
        let before = h.look().map(bar_string);
        t.set_window_back(Rows::of_count(1));
        assert!(
            h.refresh(&t, s, Chord::Cmd, Focus::Gained, NONE),
            "a frame redraws it"
        );
        assert_ne!(h.look().map(bar_string), before, "the control: it moved");
        assert_eq!(h.look(), fresh(&t, Scrolled::START).as_ref());
    }

    /// **A URI in the text under a held preview, with lines retired**:
    /// the driver's count of retired lines advances with the window, and
    /// the refresh reads the URI and its cells on the line the view shows
    /// under the pointer. Perturbation: refresh with the `back` of the last
    /// pointer event -- the bar fails. The count cancels out of what the
    /// preview reads (a line is the count less `back` plus the row, and a
    /// read finds it from the same count), so a stale one cannot be seen
    /// here; the spot carries the driver's current one, as a click's does.
    #[test]
    fn a_held_preview_of_a_uri_in_the_text_reads_the_lines_shown() {
        let mut owned = Owned::with_history(
            &WIDE,
            &baud_core::CacheGeometry::UNKNOWN,
            baud_core::Depth::of(50),
        );
        let mut t = Term::new(owned.storage(), &WIDE);
        let mut bytes = Vec::new();
        for n in 0..14 {
            bytes.extend_from_slice(format!("see https://u{n}.example/{n}\r\n").as_bytes());
        }
        feed(&mut t, &bytes);
        // The driver's count: the lines retired since the first.
        let retired = |t: &Term<'_>| Scrolled::START.advanced(Rows::of_count(t.scrollback().len()));
        t.set_window_back(Rows::of_count(3));
        let pointer = |t: &Term<'_>| Spot {
            scrolled: retired(t),
            back: t.window_back(),
            row: Row::ZERO,
            col: Col::at_index(6),
        };
        let mut h = Hover::default();
        let at = Some((Row::ZERO, Col::at_index(6)));
        assert!(h.update(&t, at, retired(&t), Chord::Cmd, Focus::Gained, NONE));
        let shown = h.look().cloned();
        feed(&mut t, b"more\r\nmore\r\n");
        let _ = h.refresh(&t, retired(&t), Chord::Cmd, Focus::Gained, NONE);
        let s = pointer(&t);
        let want = preview(&t, s, &hovered(&t, s).unwrap(), NONE);
        assert_eq!(h.look(), want.as_ref());
        assert_eq!(h.look(), shown.as_ref(), "the same URI, on the same row");
    }

    /// **Command pressed with the pointer resting shows the preview at
    /// the next frame**: a move without Command, then a frame's refresh
    /// with it held, which says the frame changed so the driver draws it.
    /// The control: let go again, the frame takes it away. Perturbation:
    /// keep the pointer's cell only while Command is held -- the press
    /// fails.
    #[test]
    fn a_press_of_command_alone_shows_on_the_next_frame() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            b"\x1b]8;;https://a.example/\x1b\\link\x1b]8;;\x1b\\",
        );
        let (at, s, key) = (Some((Row::ZERO, Col::ZERO)), Scrolled::START, Focus::Gained);
        let mut h = Hover::default();
        assert!(!h.update(&t, at, s, Chord::Other, key, NONE));
        assert!(h.refresh(&t, s, Chord::Cmd, key, NONE), "the frame changes");
        assert!(h.look().is_some(), "a press of Command alone shows");
        assert!(
            h.refresh(&t, s, Chord::Other, key, NONE),
            "the control: let go"
        );
        assert!(h.look().is_none());
        // Off the text nothing shows, and without the keyboard nothing
        // shows.
        assert!(!h.update(&t, None, s, Chord::Other, key, NONE));
        assert!(!h.refresh(&t, s, Chord::Cmd, key, NONE), "off the text");
        let _ = h.update(&t, at, s, Chord::Other, Focus::Lost, NONE);
        let _ = h.refresh(&t, s, Chord::Cmd, Focus::Lost, NONE);
        assert!(h.look().is_none(), "no keyboard, no preview");
    }

    /// **The pointing hand shows exactly while a click would open the
    /// link**: the chord held without Shift, in a key window, over a link
    /// the preview shows and [`admit`] lets open -- over a program's
    /// `OSC 22` crosshair too, and under its mouse tracking, which a click
    /// on a link never reaches. The program's shape comes back when the
    /// chord is let go, off the link, without the keyboard and when the
    /// preview is cleared; over a refused link the preview says why and the
    /// pointer is an arrow, as no link; while a left press would be the
    /// scrollbar's it keeps the program's shape. The control: with no `OSC 22`, the
    /// driver's own pointer, none. Perturbations: the hand for any
    /// previewed link -- the refused case fails; the program's shape over
    /// the hand -- the first assertion fails; the hand whoever takes a
    /// left press -- "a click scrolls" fails.
    #[test]
    fn the_hand_shows_while_a_click_would_open_the_link() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            b"\x1b]8;;https://h.example/\x1b\\web\x1b]8;;\x1b\\ \x1b]8;;ftp://f.example/x\x1b\\ftp\x1b]8;;\x1b\\ plain",
        );
        let (web, ftp, plain) = (
            Some((Row::ZERO, Col::ZERO)),
            Some((Row::ZERO, Col::at_index(4))),
            Some((Row::ZERO, Col::at_index(9))),
        );
        let (s, key) = (Scrolled::START, Focus::Gained);
        let mut h = Hover::default();
        assert_eq!(h.pointer(None), None, "the control: nothing asked");
        let _ = h.update(&t, web, s, Chord::Cmd, key, NONE);
        assert_eq!(
            h.pointer(None),
            Some(PointerShape::Hand2),
            "over an opening link"
        );
        feed(&mut t, b"\x1b]22;crosshair\x1b\\");
        let asked = t.pointer_shape();
        assert_eq!(asked, Some(PointerShape::Crosshair));
        assert_eq!(h.pointer(asked), Some(PointerShape::Hand2), "over OSC 22");
        feed(&mut t, b"\x1b[?1000h\x1b[?1006h");
        assert!(!h.refresh(&t, s, Chord::Cmd, key, NONE));
        assert_eq!(
            h.pointer(asked),
            Some(PointerShape::Hand2),
            "under tracking"
        );
        assert!(h.refresh(&t, s, Chord::Other, key, NONE));
        assert_eq!(h.pointer(asked), asked, "the chord let go");
        assert_eq!(h.pointer(None), None);
        let _ = h.update(&t, web, s, Chord::Cmd, Focus::Lost, NONE);
        assert_eq!(h.pointer(asked), asked, "no keyboard");
        let _ = h.update(&t, plain, s, Chord::Cmd, key, NONE);
        assert_eq!(h.pointer(asked), asked, "off the link");
        let _ = h.update(&t, ftp, s, Chord::Cmd, key, NONE);
        assert!(h.look().is_some(), "a refused link is previewed");
        assert_eq!(
            h.pointer(asked),
            Some(PointerShape::Arrow),
            "as no link: an arrow, over the program's shape too"
        );
        assert_eq!(h.pointer(None), Some(PointerShape::Arrow));
        let _ = h.update(&t, web, s, Chord::Cmd, key, NONE);
        assert_eq!(h.pointer(asked), Some(PointerShape::Hand2));
        assert!(h.set_left_press(LeftPress::Scrollbar), "told");
        assert_eq!(h.pointer(asked), asked, "a click scrolls");
        assert!(!h.set_left_press(LeftPress::Scrollbar), "no change");
        assert!(h.set_left_press(LeftPress::Cells));
        assert_eq!(h.pointer(asked), Some(PointerShape::Hand2));
        assert!(h.clear());
        assert_eq!(h.pointer(asked), asked, "cleared");
    }

    // --- Your `[link.*]` rules ------------------------------------------

    /// What a read under the pointer has with `rules` in force and no
    /// printed path.
    struct Ruled(Rules);

    impl PathsAt for Ruled {
        fn generation(&self) -> crate::paths::Generation {
            crate::paths::Generation::default()
        }

        fn at(
            &self,
            _: &[Unit],
            _: Ix,
            _: (Edge, Edge),
            _: (AbsLine, AbsLine),
            _: baud_core::Screen,
        ) -> Option<crate::paths::PathAt> {
            None
        }

        fn rules(&self) -> &Rules {
            &self.0
        }
    }

    /// `[link.NAME]` sections, each with its fields, read.
    fn ruled(sections: &[(&str, &[(&str, &str)])]) -> Rules {
        let sections: Vec<_> = sections
            .iter()
            .map(|(name, fields)| crate::rules::Section {
                name: name.to_string(),
                fields: fields
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            })
            .collect();
        // Read as `Config::apply` reads them, by the one function it calls.
        let (rules, said) = crate::rules::Section::read_all(&sections, None, None);
        assert_eq!(said.len(), 0, "{said:?}");
        rules
    }

    /// **A scheme a rule adds is read in the text as baud's own are**: by
    /// RFC 3986's characters, its trailing prose cut, with an authority
    /// where `//` follows, and only where a scheme may start; with no rule,
    /// nothing. Perturbation: return no added scheme from `scheme_at` --
    /// every read is none.
    #[test]
    fn a_scheme_a_rule_adds_is_read_in_the_text() {
        let r = ruled(&[("tel", &[]), ("ssh", &[])]);
        let read = |s: &str| {
            let w = win(s);
            find_with(&w, &r).map(|f| {
                f.verdict.map(|()| {
                    w.text[f.span]
                        .iter()
                        .map(|u| match u {
                            Unit::Char(c) => *c,
                            _ => '?',
                        })
                        .collect::<String>()
                })
            })
        };
        assert_eq!(
            read("call tel:+1‸5551234."),
            Some(Ok("tel:+15551234".into()))
        );
        assert_eq!(read("(ssh://h‸ost/x)"), Some(Ok("ssh://host/x".into())));
        assert_eq!(read("ssh://[::1]‸:22"), Some(Ok("ssh://[::1]:22".into())));
        assert_eq!(read("ssh://h]‸x"), Some(Err(Refused::Syntax)));
        assert_eq!(read("xtel:1‸2"), None, "not where a scheme starts");
        assert_eq!(read("sip:1‸2"), None, "no rule names it");
        assert_eq!(
            read_in(&win("call tel:+1‸555")),
            None,
            "the control: no rule"
        );
    }

    /// **E13: an `OSC 8` link is its cells', then yours, then baud's**: a cell
    /// in an `OSC 8` run whose text is itself an address belongs to the run,
    /// with a rule in force or not, so a click opens the target the program
    /// declared and never the text; a cell belongs to one link. Perturbation:
    /// read the text before the cell's `OSC 8` link in `under_with` -- the
    /// click opens the printed address.
    #[test]
    fn an_osc8_link_comes_first_and_a_cell_belongs_to_one_link() {
        let mut owned = Owned::new(&WIDE);
        let mut t = Term::new(owned.storage(), &WIDE);
        feed(
            &mut t,
            b"\x1b]8;;https://declared.example/\x1b\\tel:+1555 https://shown.example/\x1b]8;;\x1b\\",
        );
        let r = Ruled(ruled(&[("tel", &[])]));
        for col in [1, 14] {
            let s = spot(Scrolled::START, Row::ZERO, Col::at_index(col));
            let got = under_with(&t, s, Press::Down, Chord::Cmd, Focus::Gained, None, &r);
            let Some(Under::Osc8(link)) = got else {
                panic!("column {col}: {got:?}");
            };
            assert_eq!(link.uri(&t), Some(&b"https://declared.example/"[..]));
            let mut buffers = Reading::default();
            assert!(matches!(
                hovered_in(&t, s, &mut buffers, &r),
                Some(Under::Osc8(_))
            ));
        }
        // The control: the same text with no `OSC 8` is the rule's link.
        let mut owned = Owned::new(&WIDE);
        let mut t = Term::new(owned.storage(), &WIDE);
        feed(&mut t, b"tel:+1555 https://shown.example/");
        let s = spot(Scrolled::START, Row::ZERO, Col::at_index(1));
        let got = under_with(&t, s, Press::Down, Chord::Cmd, Focus::Gained, None, &r);
        let Some(Under::Bare(b)) = got else {
            panic!("{got:?}");
        };
        assert_eq!(b.uri(), Ok(&b"tel:+1555"[..]));
        let t2 = admit_with(b.uri().unwrap(), NONE, &r.0).expect("admitted");
        assert_eq!((t2.scheme(), t2.action()), (Scheme::Added, Action::Open));
    }

    /// **E9: every rule's link shows its address under the modifier, in the
    /// same bar**: the escaped address, every cell of it in the message pair
    /// no program's colours reach, and the link underlined because a click
    /// opens it -- a rule that runs a command included. Perturbation: skip the
    /// bar for an `Action::Command` link -- the look is none.
    #[test]
    fn every_rules_link_shows_its_address_in_the_bar() {
        let mut owned = Owned::new(&WIDE);
        let mut t = Term::new(owned.storage(), &WIDE);
        feed(&mut t, b"call tel:+15551234 now");
        let r = Ruled(ruled(&[(
            "tel",
            &[("command", "[\"/bin/sh\", \"{url}\"]")],
        )]));
        let mut h = Hover::default();
        let at = Some((Row::ZERO, Col::at_index(8)));
        h.update_with(&t, at, Scrolled::START, Chord::Cmd, Focus::Gained, NONE, &r);
        let look = h.look().expect("a look under the modifier");
        assert!(
            bar_string(look).contains("tel:+15551234"),
            "{}",
            bar_string(look)
        );
        assert!(look.bar.hints.iter().all(|x| *x == Hints::MESSAGE));
        assert!(
            !look.spans.is_empty(),
            "a click opens it, so it is underlined"
        );
        assert_eq!(h.pointer(None), Some(PointerShape::Hand2));
        // The control: with no rule there is nothing under the pointer.
        let mut h = Hover::default();
        h.update_with(
            &t,
            at,
            Scrolled::START,
            Chord::Cmd,
            Focus::Gained,
            NONE,
            &NoPaths,
        );
        assert_eq!(h.look(), None);
    }
}
