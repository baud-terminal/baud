//! Link activation: which link a Cmd-click opens, and which it refuses --
//! an `OSC 8` run, or a URI printed as plain text.
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
//!   an unarmed Cmd press and on the release of a press it armed, never on
//!   a drag, a move or a frame. Where the RFC is silent, practice decided,
//!   through Jev on 2026-10-18 at below 0.9 and so on the lean: trailing
//!   `.,;:!?'*` and an unmatched closing bracket are prose, not the URI; an
//!   opening `(` or `[` just before the scheme ends it at the closer that
//!   matches nothing inside; a letter outside ASCII running on from it
//!   refuses it, because the text names a page `admit` cannot open as
//!   shown. A URI that may have been cut -- past `LINK_MAX`, a `%` without
//!   two hex digits, a bracket outside an IP-literal host, or a `<` or
//!   `"` opened before it and never closed before a hard line break
//!   (Appendix C's own example) -- is refused whole, never opened
//!   shortened.
//! * [`gesture`] decides what a mouse event over a link means: Cmd-click,
//!   armed on the press and opened on the release over the same link, the
//!   whole gesture the terminal's -- a child tracking the mouse never sees
//!   it, the way Shift already carves a selection out.
//!
//! What this does not do, stated: a URI outside such a wrapper broken
//! across a hard line break is read only up to the break, and nothing marks a link until the
//! pointer-cursor and hover underline land, both of which need a
//! modifier-change callback the window crate does not carry yet.
#![forbid(unsafe_code)]

use baud_core::{Focus, LINK_MAX, Term};
use baud_domain::{Attrs, Col, LinkId, PackedCell, Row, Rows, Wrap};
use baud_render::{AbsLine, Point, Scrolled, row_of};
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
}

/// The four schemes the policy knows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scheme {
    Http,
    Https,
    Mailto,
    File,
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
    /// Not `http`, `https`, `mailto` or `file`. A leading `-` fails here
    /// too, which is what closes the argv hazard an opener would have had.
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
}

/// A URI the policy admitted, with what it is for. Only [`admit`] builds
/// one, so the opener's one call site cannot be handed a raw string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target<'a> {
    uri: &'a str,
    scheme: Scheme,
}

impl<'a> Target<'a> {
    pub fn as_str(&self) -> &'a str {
        self.uri
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    pub fn action(&self) -> Action {
        match self.scheme {
            Scheme::File => Action::Reveal,
            Scheme::Http | Scheme::Https | Scheme::Mailto => Action::Open,
        }
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

/// Decide whether `uri` may be handed to the OS, and what for.
pub fn admit<'a>(uri: &'a [u8], host: Host<'_>) -> Result<Target<'a>, Refused> {
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
    let scheme = if name.eq_ignore_ascii_case("http") {
        Scheme::Http
    } else if name.eq_ignore_ascii_case("https") {
        Scheme::Https
    } else if name.eq_ignore_ascii_case("mailto") {
        Scheme::Mailto
    } else if name.eq_ignore_ascii_case("file") {
        Scheme::File
    } else {
        return Err(Refused::Scheme);
    };
    match scheme {
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
    Ok(Target { uri: s, scheme })
}

/// One RFC 3986 character class, a bit of [`CLASSES`]' byte per ASCII code.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Class(u8);

impl Class {
    const NONE: Class = Class(0);
    /// unreserved / reserved / `%`: a character a URI may contain (§ 2).
    const URI: Class = Class(1);
    /// ALPHA / DIGIT / `+` / `-` / `.`: a scheme's (§ 3.1).
    const SCHEME: Class = Class(2);
    /// HEXDIG, what follows a `%` (§ 2.1).
    const HEX: Class = Class(4);
    /// Prose punctuation the RFC allows inside a URI and practice reads as
    /// the sentence's when it ends one: `.,;:!?'*`.
    const TRAIL: Class = Class(8);

    fn has(self, k: Class) -> bool {
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

fn class(u: Unit) -> Class {
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
struct Known {
    prefix: &'static str,
    authority: Authority,
}

const KNOWN: [Known; 4] = [
    Known {
        prefix: "https://",
        authority: Authority::Yes,
    },
    Known {
        prefix: "http://",
        authority: Authority::Yes,
    },
    Known {
        prefix: "mailto:",
        authority: Authority::No,
    },
    Known {
        prefix: "file://",
        authority: Authority::Yes,
    },
];

/// The known scheme `rest` starts with, compared without case (§ 3.1).
fn scheme_at(rest: &[Unit]) -> Option<&'static Known> {
    KNOWN.iter().find(|k| {
        k.prefix.len() <= rest.len()
            && k.prefix
                .chars()
                .zip(rest)
                .all(|(c, u)| matches!(u, Unit::Char(x) if x.eq_ignore_ascii_case(&c)))
    })
}

/// A position in a [`Window`]'s text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ix(usize);

/// Whether a [`Window`]'s end is the logical line's, or where the walk
/// stopped because a URI there would be past `LINK_MAX` anyway.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Line,
    Cut,
}

/// The text around a clicked cell: the logical line's cells, as far as
/// `MAX + 1` either side, each with the cell it was read from.
#[derive(Clone, PartialEq, Eq, Debug)]
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

/// Whether `uri`'s closers outnumber its openers: a trailing closer then
/// closes something outside it.
fn outnumbered(uri: &[Unit], open: char, close: char) -> bool {
    let opens = uri.iter().filter(|u| **u == Unit::Char(open)).count();
    let closes = uri.iter().filter(|u| **u == Unit::Char(close)).count();
    closes > opens
}

fn bracket(u: &Unit) -> bool {
    matches!(u, Unit::Char('[' | ']'))
}

/// RFC 3986 on a URI's characters, whole: every `%` begins `pct-encoded`
/// (§ 2.1), and a bracket stands only around an IP-literal host (§ 3.2.2).
fn syntax(uri: &[Unit], known: &Known) -> Result<(), Refused> {
    for (i, u) in uri.iter().enumerate() {
        if *u == Unit::Char('%') {
            let hex = |u: Option<&Unit>| u.is_some_and(|u| class(*u).has(Class::HEX));
            if !(hex(uri.get(i + 1)) && hex(uri.get(i + 2))) {
                return Err(Refused::Syntax);
            }
        }
    }
    let rest = &uri[known.prefix.len()..];
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
/// one: a URI inside another's query is the other's.
pub fn find(w: &Window) -> Option<Found> {
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
    let cut = (lo == 0 && w.head == Edge::Cut) || (hi == text.len() && w.tail == Edge::Cut);
    for p in lo..=at {
        if p > 0 && class(text[p - 1]).has(Class::SCHEME) {
            continue;
        }
        let Some(known) = scheme_at(&text[p..hi]) else {
            continue;
        };
        let body = p + known.prefix.len();
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
        if opened && w.tail == Edge::Line && text[hi..].iter().all(blank) {
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
        while end > body {
            let last = text[end - 1];
            let unmatched = (last == Unit::Char(')') && outnumbered(&text[p..end], '(', ')'))
                || (last == Unit::Char(']') && outnumbered(&text[p..end], '[', ']'));
            if class(last).has(Class::TRAIL) || unmatched {
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

/// A URI read from the text: the cells it spans, both ends inclusive, by
/// absolute line so a scroll between the press and the release keeps it,
/// and its bytes, or why it may not be opened.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Bare {
    from: Point,
    to: Point,
    text: Box<[u8]>,
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
    /// The URI around the clicked cell, with its cells.
    pub fn bare(&self) -> Option<Bare> {
        let f = find(self)?;
        let text = self.text[f.span.clone()]
            .iter()
            .filter_map(|u| match u {
                Unit::Char(c) => u8::try_from(*c).ok(),
                Unit::Marked | Unit::Gap => None,
            })
            .collect();
        Some(Bare {
            from: *self.cells.get(f.span.start)?,
            to: *self.cells.get(f.span.end.checked_sub(1)?)?,
            text,
            verdict: f.verdict,
        })
    }
}

/// What a cell reads as: its character, unless it carries marks, is
/// empty, is in an `OSC 8` run or is concealed.
fn unit(term: &Term<'_>, cell: PackedCell) -> Unit {
    if term.link_uri(cell.flags.link()).is_some() || term.style_of(cell).attrs().has(Attrs::HIDDEN)
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
    let (row, wrap) = row_of(term, scrolled, line)?;
    let mut c = col.index().min(row.len().checked_sub(1)?);
    if c > 0 && row[c].content.is_wide_tail() {
        c -= 1;
    }
    let mut left = Vec::new();
    let mut head = Edge::Line;
    let (mut l, mut cells, mut i) = (line, row, c);
    'left: loop {
        while i > 0 {
            i -= 1;
            if cells[i].content.is_wide_tail() {
                continue;
            }
            if left.len() > MAX {
                head = Edge::Cut;
                break 'left;
            }
            left.push((unit(term, cells[i]), Point::new(l, Col::at_index(i))));
        }
        let Some(up) = l.above() else { break };
        match row_of(term, scrolled, up) {
            Some((prev, Wrap::Continues)) => {
                (l, cells, i) = (up, prev, prev.len());
            }
            _ => break,
        }
    }
    let mut right = Vec::new();
    let mut tail = Edge::Line;
    let (mut l, mut cells, mut wraps, mut i) = (line, row, wrap, c);
    'right: loop {
        while i < cells.len() {
            if !cells[i].content.is_wide_tail() {
                if right.len() > MAX {
                    tail = Edge::Cut;
                    break 'right;
                }
                right.push((unit(term, cells[i]), Point::new(l, Col::at_index(i))));
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
    let at = Ix(left.len());
    let (text, cells) = left.into_iter().rev().chain(right).unzip();
    Some(Window {
        text,
        cells,
        at,
        head,
        tail,
    })
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
/// [`find`] reads around it. No other event reads the text, so moving,
/// dragging and drawing cost nothing for it: the work is a click's, and
/// bounded by `LINK_MAX` ([`window`]).
pub fn under(
    term: &Term<'_>,
    spot: Spot,
    press: Press,
    chord: Chord,
    focus: Focus,
    armed: Option<&Armed>,
) -> Option<Under> {
    let view = term.view(spot.back);
    let cell = *view.row(spot.row).get(spot.col.index())?;
    let id = cell.flags.link();
    if term.link_uri(id).is_some() {
        return Some(Under::Osc8(id));
    }
    let read = match (press, armed) {
        (Press::Down, None) => chord == Chord::Cmd && focus == Focus::Gained,
        (Press::Up, Some(a)) => matches!(a.link, Under::Bare(_)),
        _ => false,
    };
    if !read {
        return None;
    }
    let line = spot.scrolled.line(spot.back, spot.row);
    window(term, spot.scrolled, line, spot.col)?
        .bare()
        .map(Under::Bare)
}

/// What a press is over: an `OSC 8` run, or a URI in the text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Under {
    Osc8(LinkId),
    Bare(Bare),
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

/// The modifier chord of a press: Cmd alone is the link gesture, and
/// Shift wins over it -- one meaning per modifier, and Shift's is the
/// selection carve-out, which is older and documented.
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
        Under::Osc8(LinkId::from_index(3))
    }

    #[allow(non_snake_case)]
    fn M() -> Under {
        Under::Osc8(LinkId::from_index(4))
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

    /// RFC 3986 Appendix C's own example, hard-broken inside its `<`:
    /// the text names a longer address than the row holds, so the click
    /// refuses it rather than open the prefix; the same text on one line
    /// opens the whole URI, and a closed wrapper opens as before.
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
}
