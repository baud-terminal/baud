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
//! * A row the writer ended at the last column, the next starting in a URI's
//!   characters -- what a program wrapping by itself at the width leaves, as a
//!   multiplexer repainting a full-width pane a row at a time does -- is read
//!   on into the next ([`joins_below`]) while `[link] join_rows` is on and
//!   both rows are of one provenance, and what that finds is kept only where
//!   it crosses the row and admits ([`read_under`]).
//!
//! What this does not do, stated: a URI outside such a wrapper broken
//! across any other hard line break is read only up to the break. It gets no
//! line where that break is at the row's last column and the next row carries
//! on in a URI's characters ([`may_be_cut`]) but no join holds; a click there
//! still opens what is read, and the bar shows it. A program that wraps short
//! of the last column, as a pane beside another in a multiplexer does, leaves
//! no edge to see, and a URI it cut reads as whole.
#![forbid(unsafe_code)]

use baud_core::{Focus, LINK_MAX, PointerShape, Term};

#[cfg(test)]
pub use crate::paths::NoPaths;
pub use crate::paths::{Gone, PathsAt};
use crate::rules::{NO_RULES, Rules};

use baud_domain::{
    AdmittedUri, Attrs, ClusterEpoch, Codepoint, Col, Cols, DirectoryReport, FileHost, FileUri,
    LineRendition, LinkEpoch, LinkId, PackedCell, Row, Rows, Wrap,
};
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
    /// Run the program a `[link.*]` rule of yours names, in a new baud
    /// window of its own (`rules.rs`, `Rules::window`).
    Window,
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
    /// A `mailto` header other than `to`, `subject` or `body`: `bcc=` prefills a
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
    /// An `OSC 8` `file://` link naming this machine, printed while a
    /// program not known to be on it was in front -- `ssh` and its kin: the
    /// file it names is the other machine's, and this machine's file of the
    /// same name is not the one meant ([`crate::paths::Origin::Remote`]).
    Remote,
    /// The same, printed before the worker named the program in front: a
    /// click a moment later finds it named ([`crate::paths::Origin::Unknown`]).
    Unnamed,
}

/// A URI the policy admitted, with what it is for. Only [`admit`] builds
/// one, so the opener's call sites cannot be handed a raw string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target<'a> {
    uri: &'a str,
    scheme: Scheme,
    /// The `[link.*]` rule that took it, by its place, and what it does.
    rule: Option<(crate::rules::RuleIx, Action)>,
    /// A `file:` link's file, as the policy read it.
    file: Option<LocalFile<'a>>,
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

    /// **A `file:` link's file, as the policy read it**: none for any
    /// other scheme.
    pub fn local_file(&self) -> Option<LocalFile<'a>> {
        self.file
    }

    /// **What a reveal hands the file manager**: a `file:` link's file
    /// with no host ([`LocalFile::local_uri`]), any other URI as admitted.
    pub fn reveal_uri(&self) -> AdmittedUri {
        AdmittedUri::checked(match self.file {
            Some(f) => f.local_uri(),
            None => self.uri.to_owned(),
        })
    }

    /// **What a handler or a command of yours is handed**: a `mailto:` link
    /// rebuilt from its parse ([`MailtoUri::to_uri`]), so the mail client
    /// gets exactly the recipients and headers the policy read; any other
    /// URI as admitted. The preview shows it too.
    pub fn handed(&self) -> std::borrow::Cow<'a, str> {
        let rebuilt = match self.scheme {
            Scheme::Mailto => self
                .uri
                .split_once(':')
                .and_then(|(_, rest)| MailtoUri::parse(rest).ok())
                .map(|m| m.to_uri()),
            _ => None,
        };
        rebuilt.map_or(
            std::borrow::Cow::Borrowed(self.uri),
            std::borrow::Cow::Owned,
        )
    }
}

/// **A `file:` link's file, as RFC 8089's grammar reads it**
/// ([`FileUri`], `[[source: rfc8089]]` § 2): its host, this machine by no
/// name or by one of its names, and its path, still percent-encoded, every
/// byte on the grammar. Only the policy makes one, from the URI it admits,
/// so the paths and the opener read the file the policy judged and never
/// parse the text a second time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocalFile<'a>(FileUri<'a>);

impl LocalFile<'_> {
    /// The path, percent-decoded: none where it decodes to a NUL, which no
    /// POSIX path can hold.
    pub fn path(&self) -> Option<Vec<u8>> {
        let mut out = vec![0; self.0.encoded_path().len()];
        let n = self.0.decode_path(&mut out)?.len();
        out.truncate(n);
        Some(out)
    }

    /// **The URI a file manager is handed**: the path as written, with no
    /// authority, as RFC 8089 § 2 suggests "To maximize compatibility",
    /// since "Some current usages of the scheme incorrectly interpret all
    /// values in the authority of a file URI, including "localhost", as
    /// non-local". Every byte of the path is ASCII, being on the grammar.
    pub fn local_uri(&self) -> String {
        let path = String::from_utf8_lossy(self.0.encoded_path());
        format!("file://{path}")
    }
}

/// **An `http` or `https` URI's host, never empty**: "A sender MUST NOT
/// generate an "http" URI with an empty host identifier. A recipient that
/// processes such a URI reference MUST reject it as invalid"
/// (`[[source: rfc9110]]` § 4.2.1). The authority is split as RFC 3986
/// § 3.2 has it, `[ userinfo "@" ] host [ ":" port ]` (`[[source:
/// rfc3986]]`): the host is after the last `@`, which userinfo cannot hold
/// unencoded, and before the port's `:`, an IP-literal's brackets kept
/// whole, so `http://[::1]:80/` names `[::1]`. Finer checks of the host
/// are the system opener's. The host of `rest`, what follows the scheme's
/// `:`; refused as [`Refused::Host`] with no authority, an empty host, or an
/// IP-literal not closed.
fn http_host(rest: &str) -> Result<&str, Refused> {
    let auth = rest.strip_prefix("//").ok_or(Refused::Host)?;
    let auth = &auth[..auth.find(['/', '?', '#']).unwrap_or(auth.len())];
    let hostport = auth.rfind('@').map_or(auth, |at| &auth[at + 1..]);
    let host = match hostport.strip_prefix('[') {
        Some(literal) => {
            let close = literal.find(']').ok_or(Refused::Host)?;
            &hostport[..close + 2]
        }
        None => &hostport[..hostport.find(':').unwrap_or(hostport.len())],
    };
    match host {
        "" | "[]" => Err(Refused::Host),
        _ => Ok(host),
    }
}

/// The headers a `mailto:` link may carry: its recipients and the
/// composer's subject and body. `bcc=` prefills a hidden recipient, and any
/// other header reaches whatever application is registered for `mailto`,
/// which is not necessarily a composer, so the rest are refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HeaderName {
    To,
    Subject,
    Body,
}

impl HeaderName {
    fn of(name: &str) -> Result<HeaderName, Refused> {
        [
            ("to", HeaderName::To),
            ("subject", HeaderName::Subject),
            ("body", HeaderName::Body),
        ]
        .into_iter()
        .find(|(n, _)| name.eq_ignore_ascii_case(n))
        .map(|(_, h)| h)
        .ok_or(Refused::Header)
    }

    fn name(self) -> &'static str {
        match self {
            HeaderName::To => "to",
            HeaderName::Subject => "subject",
            HeaderName::Body => "body",
        }
    }
}

/// **A `mailto:` URI as baud parsed it**, `[[source: rfc6068]]` § 2:
/// `mailtoURI = "mailto:" [ to ] [ hfields ]`, `hfields = "?" hfield *(
/// "&" hfield )`, `hfield = hfname "=" hfvalue`. Its recipients in the
/// path, which may be empty when a `to` header names them, and its
/// headers, each split at its first `=`. Its only output is
/// [`MailtoUri::to_uri`], rebuilt from the parse with every value
/// percent-encoded, so the mail client is handed exactly what baud parsed:
/// `subject=x?bcc=e@x` is one subject, never a hidden recipient.
#[derive(Clone, PartialEq, Eq, Debug)]
struct MailtoUri<'a> {
    to: &'a str,
    headers: Vec<(HeaderName, &'a str)>,
}

impl<'a> MailtoUri<'a> {
    /// `rest`, what follows `mailto:`, parsed: refused as
    /// [`Refused::Header`] for a header not in [`HeaderName`], or for a
    /// control (C0 or `DEL`) the recipients or a `to` or `subject` value
    /// decode to -- `%0D%0A` there would end the header and begin another
    /// in the message, `Bcc:` among them; only the body may carry a line
    /// break, which "MUST be encoded with "%0D%0A"", and "Line breaks in
    /// other <hfield>s SHOULD NOT be used" (RFC 6068 § 5) -- and as
    /// [`Refused::Host`] when nothing names a recipient.
    fn parse(rest: &'a str) -> Result<MailtoUri<'a>, Refused> {
        let (to, query) = match rest.split_once('?') {
            Some((to, q)) => (to, Some(q)),
            None => (rest, None),
        };
        let controls = |s: &str| {
            percent_decoded(s.as_bytes())
                .iter()
                .any(|&b| b < 0x20 || b == 0x7f)
        };
        if controls(to) {
            return Err(Refused::Header);
        }
        let mut headers = Vec::new();
        for field in query.into_iter().flat_map(|q| q.split('&')) {
            let (name, value) = field.split_once('=').unwrap_or((field, ""));
            let name = HeaderName::of(name)?;
            if name != HeaderName::Body && controls(value) {
                return Err(Refused::Header);
            }
            headers.push((name, value));
        }
        let named = !to.is_empty()
            || headers
                .iter()
                .any(|&(h, v)| h == HeaderName::To && !v.is_empty());
        match named {
            true => Ok(MailtoUri { to, headers }),
            false => Err(Refused::Host),
        }
    }

    /// **The URI handed on**: the recipients as written, then each header
    /// by its name, its value percent-decoded and encoded again with every
    /// byte but RFC 3986's unreserved ones escaped -- each a `qchar`, a
    /// `pct-encoded` one where it is not unreserved -- so no value can
    /// close its field or begin another.
    fn to_uri(&self) -> String {
        let mut out = format!("mailto:{}", self.to);
        for (i, (name, value)) in self.headers.iter().enumerate() {
            out.push(match i {
                0 => '?',
                _ => '&',
            });
            out.push_str(name.name());
            out.push('=');
            for b in percent_decoded(value.as_bytes()) {
                match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                        out.push(char::from(b));
                    }
                    _ => out.push_str(&format!("%{b:02X}")),
                }
            }
        }
        out
    }
}

/// `bytes` with each `%` and two hex digits decoded, and a `%` without
/// them kept as it is.
fn percent_decoded(bytes: &[u8]) -> Vec<u8> {
    let hex = |b: u8| char::from(b).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let pair = (bytes.get(i + 1).copied(), bytes.get(i + 2).copied());
        match (bytes[i], pair) {
            (b'%', (Some(h), Some(l))) if hex(h).is_some() && hex(l).is_some() => {
                let v = hex(h).zip(hex(l)).map_or(0, |(h, l)| h << 4 | l);
                out.push(u8::try_from(v).unwrap_or(0));
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// **An admitted URI that owns its bytes, and what it is for**: what
/// [`admit_owned`] gives back, to move into a later turn, where a
/// [`Target`] borrows and cannot go.
#[derive(Debug)]
pub struct Admitted {
    uri: AdmittedUri,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    scheme: Scheme,
    action: Action,
}

impl Admitted {
    /// About's log names it (`app.rs`); Linux's driver has no About.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    pub fn action(&self) -> Action {
        self.action
    }

    /// The URI, moved out.
    pub fn into_uri(self) -> AdmittedUri {
        self.uri
    }
}

/// **[`admit_with`], keeping the `String` it checked**: the same checks, and
/// the URI moved into the answer rather than borrowed, so it can cross to
/// a later turn with no copy. Reached only through a door named for where
/// the URI came from ([`admit_path_owned`], [`admit_about`]).
fn admit_owned(uri: String, host: &MachineNames) -> Result<Admitted, Refused> {
    let (scheme, action) = {
        let t = admit_with(uri.as_bytes(), host, &NO_RULES)?;
        (t.scheme(), t.action())
    };
    Ok(Admitted {
        uri: AdmittedUri::checked(uri),
        scheme,
        action,
    })
}

/// **[`admit_path`], owned**: the `file://` URI baud built for a printed
/// path a click asked about again and the worker found on this machine.
pub fn admit_path_owned(uri: String, host: &MachineNames) -> Result<Admitted, Refused> {
    admit_owned(uri, host)
}

/// **The address the About window's picture opens**: one baud ships, read
/// from no screen and printed by no program, judged by the policy alone.
/// Its scheme is `https`, which the policy judges without this machine's
/// name, so none is passed.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn admit_about(url: String) -> Result<Admitted, Refused> {
    admit_owned(url, &crate::link::MachineNames::NONE)
}

impl Target<'_> {
    /// **The admitted URI, owned, as it is handed on** ([`Target::handed`]):
    /// its one copy, made where the bytes are
    /// borrowed from the terminal's link table or a line's text, which a
    /// later turn may find changed.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn to_admitted(self) -> AdmittedUri {
        AdmittedUri::checked(self.handed().into_owned())
    }
}

/// **This machine's names**, for a `file://host/path` link -- the form
/// `ls --hyperlink` emits -- or a shell's `OSC 7` to be recognised as
/// local: "a fully qualified domain name that resolves to the machine from
/// which the URI is being interpreted" (`[[source: rfc8089]]` § 3), which
/// only the machine can say. Each name is one the system gave, none empty,
/// and a host matches one without case. None known admits only the empty
/// and `localhost` hosts, which name no machine. The one type every reader
/// of a `file:` host is handed, so none compares against a string of its
/// own.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct MachineNames {
    names: Vec<String>,
}

impl MachineNames {
    /// No name known.
    pub const NONE: MachineNames = MachineNames { names: Vec::new() };

    /// `names`, the empty ones left out.
    pub fn of(names: impl IntoIterator<Item = String>) -> MachineNames {
        MachineNames {
            names: names.into_iter().filter(|n| !n.is_empty()).collect(),
        }
    }

    /// The one name `name`, or none if it is empty.
    /// Linux's, which reads one name; the tests'.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn one(name: &str) -> MachineNames {
        MachineNames::of([name.to_owned()])
    }

    /// Whether `host` is one of the names, without case.
    pub fn is_this(&self, host: &[u8]) -> bool {
        self.names
            .iter()
            .any(|n| host.eq_ignore_ascii_case(n.as_bytes()))
    }
}

/// **This machine's names, handed to the links and the paths as one
/// value**: the policy a click and a line's look admit by, and the paths a
/// struck file link and a folder report are read by, so a line never
/// promises a link its click refuses. Every driver hands its names over
/// here.
pub fn hand_names(
    names: MachineNames,
    uris: &mut crate::lined::Uris,
    paths: &mut crate::paths::Paths,
) {
    uris.set_names(names.clone());
    paths.set_names(names);
}

/// The longest URI the policy admits: the core's own cap, so a URI the
/// core stored whole is never refused for length here.
pub const MAX: usize = LINK_MAX;

/// Decide whether `uri` may be handed to the OS, and what for, by baud's own
/// list alone: the policy's own tests. Code reads through a door named for
/// where its URI came from ([`admit_link`], [`admit_path`], [`admit_about`]).
#[cfg(test)]
pub fn admit<'a>(uri: &'a [u8], host: &MachineNames) -> Result<Target<'a>, Refused> {
    admit_with(uri, host, &NO_RULES)
}

/// **[`admit`], with your `[link.*]` rules**: a scheme a rule names is let
/// through, the first rule in the file naming it deciding what a click
/// does. Nothing a rule says loosens the rest: the bytes and the length are
/// checked first; a default scheme keeps its own rules whichever handler
/// takes it -- `file` names this machine or none, `mailto` carries only
/// its recipients, `subject` and `body`, and `http` names a host; and a link a rule routes has no part that starts
/// with `-` ([`crate::rules::dashless`]).
fn admit_with<'a>(
    uri: &'a [u8],
    host: &MachineNames,
    rules: &Rules,
) -> Result<Target<'a>, Refused> {
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
                crate::rules::Act::Window(_) => Action::Window,
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
    let mut file = None;
    match scheme {
        Scheme::Added => {
            if rest.is_empty() {
                return Err(Refused::Host);
            }
        }
        Scheme::Http | Scheme::Https => {
            http_host(rest)?;
        }
        Scheme::Mailto => {
            MailtoUri::parse(rest)?;
        }
        // RFC 8089's grammar, read once (`FileUri`): no query, no fragment,
        // no broken escape, a path that is absolute. `file:////srv/x`, a
        // UNC host written as a path, is off it and refused.
        Scheme::File => {
            let f = match DirectoryReport::parse(uri) {
                DirectoryReport::File(f) => f,
                DirectoryReport::OtherScheme | DirectoryReport::Malformed => {
                    return Err(Refused::Syntax);
                }
            };
            match f.host {
                FileHost::Local => {}
                FileHost::Named(h) if host.is_this(h) => {}
                FileHost::Named(_) => return Err(Refused::Host),
            }
            file = Some(LocalFile(f));
        }
    }
    Ok(Target {
        uri: s,
        scheme,
        rule: ruled,
        file,
    })
}

/// **Where a link read off the screen came from**, which decides what it
/// may open: every URI read from the cells, marked with `OSC 8` or written
/// as plain text, with the origin of the program that printed it
/// ([`crate::paths::Paths::printed`]); or a printed path, which its own
/// check found on this machine and which carries none. There is no
/// default: a reader with no worker to have named a writer says what it
/// takes the line for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Printed {
    Link { origin: crate::paths::LinkOrigin },
    Path,
}

/// **A URI read off the screen, judged as a click on it is**: a link,
/// marked with `OSC 8` or written as plain text, is printed by some
/// program, and a `file://` one names a file on that program's machine.
/// One [`admit_with`] lets through names this machine -- the empty host,
/// `localhost` or one of its names -- so it is refused unless the program
/// that printed it was this machine's: `origin` is the link's line's, as
/// the worker named it when the line was written, never what is in front
/// at the click ([`crate::paths::Paths::printed`]). Its look at rest, the
/// look under the pointer, the hand, the preview and the click all ask
/// this, so none of them says a click opens what it refuses.
pub fn admit_link<'a>(
    uri: &'a [u8],
    host: &MachineNames,
    rules: &Rules,
    origin: crate::paths::LinkOrigin,
) -> Result<Target<'a>, Refused> {
    use crate::paths::Origin;
    let t = admit_with(uri, host, rules)?;
    match (t.scheme(), origin.origin()) {
        (Scheme::File, Origin::Remote) => Err(Refused::Remote),
        (Scheme::File, Origin::Unknown) => Err(Refused::Unnamed),
        _ => Ok(t),
    }
}

/// **The `file://` URI baud built for a printed path**, which the worker
/// found on this machine: the path's own check stands for its origin
/// (`paths::Paths::readable_on`), and the policy judges the rest.
pub fn admit_path<'a>(uri: &'a [u8], host: &MachineNames) -> Result<Target<'a>, Refused> {
    admit_with(uri, host, &NO_RULES)
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
    /// The rows the writer ended that the read joined to the next
    /// ([`joins_below`]), each by its line.
    joined: Vec<AbsLine>,
}

impl Window {
    /// Whether a link from `from` to `to` runs across a row the read joined.
    fn crosses(&self, from: Point, to: Point) -> bool {
        self.joined.iter().any(|&l| from.line <= l && l < to.line)
    }
}

/// **Whether a row the writer ended may be read on into the next**, as
/// `[link] join_rows` and the rows' provenance say ([`PathsAt::joins`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Joins {
    Yes,
    No,
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
/// reads what is written and the bar shows it. Where the rows join
/// ([`joins_below`]) the read runs on past the cut, and only a URI that ends
/// at such a row anyway, the join read short of it, is met here. What this
/// cannot see, stated: a program that wraps short of the last column, as a
/// pane beside another in a multiplexer does.
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

/// **Whether the row at `above`, which its writer ended, runs on into the
/// next in a URI's characters**: it ends by the writer's own line break and
/// not a soft wrap, its last cell is a URI's character, so it reaches the last
/// column, and the next row's first cell is one too. What a program wrapping
/// by itself at the width leaves, as a multiplexer repainting a full-width
/// pane a row at a time does; a row ending short of the last column, or a next
/// row starting in a space, never runs on.
pub(crate) fn runs_on(term: &Term<'_>, scrolled: Scrolled, above: AbsLine) -> bool {
    let Some((cells, wrap)) = row_of(term, scrolled, above) else {
        return false;
    };
    if wrap == Wrap::Continues {
        return false;
    }
    let below = above.below();
    let Some((next, _)) = row_of(term, scrolled, below) else {
        return false;
    };
    let uri = |line: AbsLine, cell: PackedCell| {
        class(unit(term, row_styles_of(term, scrolled, line), cell)).has(Class::URI)
    };
    cells.last().is_some_and(|&c| uri(above, c)) && next.first().is_some_and(|&c| uri(below, c))
}

/// **Whether a read joins the row at `above` to the next** ([`runs_on`]),
/// as `paths` lets it: `[link] join_rows` on, and both rows of one
/// provenance. A read keeps what it finds across such a row only where it
/// crosses the row and admits ([`read_under`], `lined.rs`), and otherwise
/// reads as if never joined.
pub(crate) fn joins_below(
    term: &Term<'_>,
    scrolled: Scrolled,
    above: AbsLine,
    paths: &dyn PathsAt,
) -> bool {
    runs_on(term, scrolled, above) && paths.joins(above) == Joins::Yes
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
    /// The first and last line its text was read from: the rows whose
    /// writers its origin is judged by.
    pub fn lines(&self) -> (AbsLine, AbsLine) {
        (self.from.line, self.to.line)
    }

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
    /// through [`under_with`] and [`Hover`], so only tests call it.
    #[cfg(test)]
    pub fn bare(&self) -> Option<Bare> {
        self.bare_in(&mut Vec::new(), &NO_RULES)
    }

    /// [`Window::bare`], its bytes in `text`'s buffer, taken only for a URI,
    /// a scheme a rule of yours added read too.
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
/// what a short one does. The drivers read through [`under_with`] and
/// [`Hover`], which join the rows [`joins_below`] lets them, so only tests
/// call it.
#[cfg(test)]
pub fn window(term: &Term<'_>, scrolled: Scrolled, line: AbsLine, col: Col) -> Option<Window> {
    let mut read = Reading::default();
    read_window(term, scrolled, line, col, &mut read, None)?;
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
    /// The first and last line the reads since it was cleared went over:
    /// what the hover keeps a copy of ([`Covered`]).
    visited: Option<(AbsLine, AbsLine)>,
}

/// The lines `visited` holds, `line` among them.
fn visit(visited: &mut Option<(AbsLine, AbsLine)>, line: AbsLine) {
    *visited = Some(match *visited {
        None => (line, line),
        Some((a, b)) => (a.min(line), b.max(line)),
    });
}

/// [`window`], into `read`'s window. With `across`, a row the writer ended is read on into the
/// next where [`joins_below`] says, and each such row is kept in the window.
fn read_window(
    term: &Term<'_>,
    scrolled: Scrolled,
    line: AbsLine,
    col: Col,
    read: &mut Reading,
    across: Option<&dyn PathsAt>,
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
        visited,
        ..
    } = read;
    left.clear();
    right.clear();
    window.joined.clear();
    let joins = |above: AbsLine| across.is_some_and(|p| joins_below(term, scrolled, above, p));
    let mut head = Edge::Line;
    let (mut l, mut cells, mut i) = (line, row, c);
    'left: loop {
        visit(visited, l);
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
            Some((prev, Wrap::Ended)) if joins(up) => {
                window.joined.push(up);
                (l, cells, i) = (up, prev, prev.len());
            }
            _ => break,
        }
    }
    let mut tail = Edge::Line;
    let (mut l, mut cells, mut wraps, mut i) = (line, row, wrap, c);
    'right: loop {
        visit(visited, l);
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
            if !joins(l) {
                break;
            }
            window.joined.push(l);
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
    let link = Osc8::at(term, spot, cell);
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
    read_under(term, spot, &mut Reading::default(), paths)
}

/// **A printed path under a click or the pointer**: the path at the window's
/// cell that `paths` has an answer for, its cells, and the URI a click hands
/// [`admit`]. One the worker found gone is refused as [`Refused::NotFound`],
/// so the bar says so and nothing opens.
fn printed_in(term: &Term<'_>, w: &Window, paths: &dyn PathsAt) -> Option<PrintedPath> {
    let lines = (w.cells.first()?.line, w.cells.last()?.line);
    let at = paths.at(&w.text, w.at, (w.head, w.tail), lines, term.screen())?;
    let from = *w.cells.get(at.span.start)?;
    let to = *w.cells.get(at.span.end.checked_sub(1)?)?;
    let uri = crate::paths::file_uri(&at.path);
    let verdict = match at.probe {
        crate::paths::Probe::Exists => {
            admit_path(uri.as_bytes(), &crate::link::MachineNames::NONE).map(|_| ())
        }
        crate::paths::Probe::Missing | crate::paths::Probe::Unknown => Err(Refused::NotFound),
    };
    Some(PrintedPath {
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
pub struct PrintedPath {
    from: Point,
    to: Point,
    path: Vec<u8>,
    form: crate::paths::Form,
    /// The `:line:col` printed after it, for a command of yours.
    line_col: crate::paths::LineCol,
    uri: String,
    verdict: Result<(), Refused>,
}

impl PrintedPath {
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
    Path(PrintedPath),
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
///
/// It keeps the view it was read in, whose rows holding its slot are the
/// cells its look draws, and so the rows its origin is judged over
/// ([`crate::paths::Paths::link_origin`]); two are the same link whatever
/// view each was read in.
#[derive(Clone, Copy, Debug)]
pub struct Osc8 {
    id: LinkId,
    epoch: LinkEpoch,
    view: (Scrolled, Rows),
}

/// The same slot under the same epoch.
impl PartialEq for Osc8 {
    fn eq(&self, other: &Osc8) -> bool {
        (self.id, self.epoch) == (other.id, other.epoch)
    }
}

impl Eq for Osc8 {}

impl Osc8 {
    /// The link `cell`, at `spot`, names, as read now, in the view it is
    /// read in.
    pub fn at(term: &Term<'_>, spot: Spot, cell: PackedCell) -> Osc8 {
        Osc8 {
            id: cell.flags.link(),
            epoch: term.link_epoch(),
            view: (spot.scrolled, spot.back),
        }
    }

    /// Its slot.
    pub fn id(&self) -> LinkId {
        self.id
    }

    /// **The rows of its view holding its slot**, top first: every cell its
    /// look draws is on one ([`crate::lined::SlotRows`]).
    pub fn view_rows(&self, term: &Term<'_>) -> Vec<AbsLine> {
        let (scrolled, back) = self.view;
        crate::lined::SlotRows::of(term, scrolled, back, Some(self.id))
            .rows(self.id)
            .to_vec()
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
        Refused::Remote => "a file on the computer the program that printed it runs on",
        Refused::Unnamed => "not known yet which computer printed it",
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
pub fn preview_text(
    link: &Under,
    term: &Term<'_>,
    host: &MachineNames,
    room: Cols,
) -> Option<String> {
    let printed = NoPaths.printed(term, link);
    preview_text_with(link, term, host, room, &NoPaths, printed)
}

/// [`preview_text`], with what `paths` found gone said as the reason, the
/// link admitted with the rules `paths` holds -- a link a rule takes
/// shows its address as any other does -- and as `printed`, with where the
/// program that printed it ran ([`crate::paths::Paths::printed`]): a
/// `file://` link a click refuses for it says it won't open.
pub fn preview_text_with(
    link: &Under,
    term: &Term<'_>,
    host: &MachineNames,
    room: Cols,
    paths: &dyn PathsAt,
    printed: Printed,
) -> Option<String> {
    let room = room.index();
    let rules = paths.rules();
    let (uri, verdict) = verdict(link, term, host, paths, printed)?;
    let admitted = match link {
        Under::Path(_) => None,
        Under::Osc8(_) | Under::Bare(_) => admit_link(uri, host, rules, origin_of(printed)).ok(),
    };
    // What a click hands on: a `mailto:` link as it is rebuilt.
    let address = match admitted {
        Some(t) => escaped(t.handed().as_bytes()),
        None => escaped(uri),
    };
    // A click that copies says so: it opens nothing.
    let copies = match link {
        Under::Path(_) => *rules.path_act() == crate::rules::PathAct::Copy,
        Under::Osc8(_) | Under::Bare(_) => admitted.is_some_and(|t| t.action() == Action::Copy),
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
/// link's URI, or the text of a URI read from the cells, and the answer for
/// it as a click's ([`admit_link`], with where the program that printed it
/// ran, `printed`), then what `paths` found gone of one it
/// admits; none for an `OSC 8` slot whose link is gone. The look under
/// the pointer, the hand and the preview read this, so none says a click
/// opens what it refuses.
fn verdict<'a>(
    link: &'a Under,
    term: &'a Term<'_>,
    host: &MachineNames,
    paths: &dyn PathsAt,
    printed: Printed,
) -> Option<(&'a [u8], Result<(), Refused>)> {
    let rules = paths.rules();
    let admitted = |uri: &[u8]| {
        let t = admit_link(uri, host, rules, origin_of(printed))?;
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
/// `paths` says of a link [`admit_link`] lets through as `printed`.
pub fn gone_of(
    link: &Under,
    term: &Term<'_>,
    host: &MachineNames,
    paths: &dyn PathsAt,
    printed: Printed,
) -> Option<Gone> {
    let admitted = |uri| admit_link(uri, host, paths.rules(), origin_of(printed)).ok();
    match link {
        Under::Path(p) => match p.verdict {
            Err(Refused::NotFound) => Some(Gone::File(p.path.clone())),
            _ => None,
        },
        Under::Osc8(o) => paths.gone(&admitted(o.uri(term)?)?),
        Under::Bare(b) => paths.gone(&admitted(b.uri().ok()?)?),
    }
}

/// The origin a link `printed` carries; a printed path carries none, and
/// a URI judged as one is not known.
fn origin_of(printed: Printed) -> crate::paths::LinkOrigin {
    match printed {
        Printed::Link { origin } => origin,
        Printed::Path => crate::paths::LinkOrigin::unknown(),
    }
}

/// **What a click on a link does with it**, as [`click_on`] decides.
#[derive(Debug)]
pub enum ClickOn<'l, 'a> {
    /// A printed path: asked about again off the frame.
    Path(&'l PrintedPath),
    /// An `OSC 8` link whose slot a sweep freed after it was read: its
    /// index may name another target now.
    Gone,
    /// A URI read from the text that is no URI, with the text.
    Unreadable(Refused, &'a [u8]),
    /// A URI refused, with the URI.
    Refused(Refused, &'a [u8]),
    /// A URI let through, with the URI as read.
    Admitted(Target<'a>, &'a [u8]),
}

/// **The one decision a click on `link` makes**, the same on both drivers:
/// a printed path is asked about again; an `OSC 8` link reads its URI from
/// the link table, and a URI read from the text its own; and either is
/// judged by [`admit_link`] with the origin of the programs that printed the
/// rows it covers ([`PathsAt::printed`]), the answer its look at rest took,
/// never what is in front at the click.
pub fn click_on<'l: 'a, 'a>(
    link: &'l Under,
    term: &'a Term<'_>,
    host: &MachineNames,
    rules: &Rules,
    paths: &dyn PathsAt,
) -> ClickOn<'l, 'a> {
    let uri = match link {
        Under::Path(p) => return ClickOn::Path(p),
        Under::Osc8(l) => match l.uri(term) {
            Some(uri) => uri,
            None => return ClickOn::Gone,
        },
        Under::Bare(b) => match b.uri() {
            Ok(uri) => uri,
            Err(why) => return ClickOn::Unreadable(why, b.text()),
        },
    };
    match admit_link(uri, host, rules, origin_of(paths.printed(term, link))) {
        Ok(t) => ClickOn::Admitted(t, uri),
        Err(why) => ClickOn::Refused(why, uri),
    }
}

/// [`click_on`], then [`click`] on what it lets through: what a click on a
/// link does, for a test to read. A printed path, or an `OSC 8` slot gone,
/// is not a link these tests click.
#[cfg(test)]
pub fn clicked(
    link: &Under,
    term: &Term<'_>,
    host: &MachineNames,
    rules: &Rules,
    paths: &dyn PathsAt,
) -> Result<Click, Refused> {
    match click_on(link, term, host, rules, paths) {
        ClickOn::Admitted(t, _) => Ok(click(&t, paths)),
        ClickOn::Refused(why, _) | ClickOn::Unreadable(why, _) => Err(why),
        ClickOn::Path(_) | ClickOn::Gone => panic!("not a link a test clicks: {link:?}"),
    }
}

/// What a click on a link `admit_with` let through does.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Click {
    /// A rule's `copy`: the link's text to the clipboard.
    Copy,
    /// A rule's command, with the link.
    Command,
    /// A rule's window, running its program with the link.
    Window,
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
        Action::Window => Click::Window,
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
    let link = Osc8::at(term, spot, cell);
    if link.uri(term).is_some() {
        return Some(Under::Osc8(link));
    }
    read_under(term, spot, read, paths)
}

/// **The URI or printed path the text at `spot` reads as**, a click's and
/// the pointer's one read. Read first across the rows the writer ended that
/// [`joins_below`] joins; what that finds is kept only where it crosses a
/// joined row and admits, so a URI the join would cut short, swallow into a
/// word or refuse reads as it did unjoined, by a second read.
fn read_under(
    term: &Term<'_>,
    spot: Spot,
    read: &mut Reading,
    paths: &dyn PathsAt,
) -> Option<Under> {
    let line = spot.scrolled.line(spot.back, spot.row);
    read_window(term, spot.scrolled, line, spot.col, read, Some(paths))?;
    if !read.window.joined.is_empty() {
        let joined = found_under(term, read, paths);
        let kept = match &joined {
            Some(Under::Bare(b)) => b.verdict.is_ok() && read.window.crosses(b.from, b.to),
            Some(Under::Path(p)) => p.verdict.is_ok() && read.window.crosses(p.from, p.to),
            Some(Under::Osc8(_)) | None => false,
        };
        if kept {
            return joined;
        }
        // The bytes a URI read across the join took go back to the buffer.
        if let Some(Under::Bare(b)) = joined {
            read.text = b.text;
        }
        read_window(term, spot.scrolled, line, spot.col, read, None)?;
    }
    found_under(term, read, paths)
}

/// The URI in `read`'s window, or else the printed path `paths` answers for.
fn found_under(term: &Term<'_>, read: &mut Reading, paths: &dyn PathsAt) -> Option<Under> {
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
        | Under::Path(PrintedPath {
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
pub fn preview(term: &Term<'_>, spot: Spot, link: &Under, host: &MachineNames) -> Option<LinkLook> {
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
    host: &MachineNames,
    paths: &dyn PathsAt,
) -> Option<LinkLook> {
    let view = term.view(spot.back);
    let (rows, cols) = (view.rows().index(), view.cols().index());
    let printed = paths.printed(term, link);
    let text = preview_text_with(
        link,
        term,
        host,
        Cols::of_index(cols.saturating_sub(2)),
        paths,
        printed,
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
    let opens = matches!(verdict(link, term, host, paths, printed), Some((_, Ok(()))));
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
/// same ones answers the same, so a read owed over a still screen reads no
/// text.
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
    host: MachineNames,
    /// The printed paths' answers, and the lines' provenance, which move
    /// with no byte.
    paths: (crate::paths::Generation, crate::paths::Generation),
}

impl ReadOf {
    fn matches(
        &self,
        term: &Term<'_>,
        (at, scrolled, chord, focus): (Option<(Row, Col)>, Scrolled, Chord, Focus),
        host: &MachineNames,
        paths: (crate::paths::Generation, crate::paths::Generation),
    ) -> bool {
        self.at == at
            && self.scrolled == scrolled
            && self.chord == chord
            && self.focus == focus
            && self.back == term.window_back()
            && self.geometry == term.geometry()
            && self.bytes == Taken(term.amplification().0)
            && self.epoch == term.link_epoch()
            && self.host == *host
            && self.paths == paths
    }
}

/// **Whether the link under a resting pointer is owed a read**: set by what
/// a read depends on that an event delivers -- the focus, a worker's answer,
/// this machine's names, the config -- and by a change to what the last read
/// covered ([`Hover::under_moved`]); a driver reads the hover on a turn only
/// when it is set.
/// The macOS driver's: the Linux driver reads at each frame.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HoverOwed {
    #[default]
    No,
    Yes,
}

/// **What the last read of the text under the pointer went over**, as it
/// then was.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
enum Text {
    /// No text was read: no chord and no look at rest asked for one, or
    /// the pointer's row is past the grid. Only an event changes that.
    #[default]
    Unread,
    /// An `OSC 8` link: its cells are every cell naming its slot on the
    /// view, which a write to any row may add to, so a byte taken or a
    /// moved window owes a read.
    Slot,
    /// The text around the pointer: the lines the read went over and the
    /// one past each end, which a URI's run-on and cut tests read
    /// (`runs_on`, `may_be_cut`), from `first`; the pointer's line and its
    /// rendition, which places the pointer's character column and decides
    /// a link drawn whole; the cluster epoch, under which equal cells hold
    /// equal text.
    Lines {
        first: AbsLine,
        line: AbsLine,
        rendition: LineRendition,
        clusters: ClusterEpoch,
    },
}

/// **[`Text`], with the cells and wrap of each line it covers**, a line
/// past the history's or the screen's end held as none: the copy a turn's
/// output is compared with, the idiom of `lined.rs`'s rows.
#[derive(Debug, Default)]
struct Covered {
    text: Text,
    cells: Vec<PackedCell>,
    rows: Vec<Option<(usize, Wrap)>>,
}

impl Covered {
    /// The copy of what a read just went over: `text`, and for its lines,
    /// through `last`, the cells and wraps the term holds now.
    fn take(&mut self, term: &Term<'_>, scrolled: Scrolled, text: Text, last: Option<AbsLine>) {
        self.cells.clear();
        self.rows.clear();
        if let (Text::Lines { first, .. }, Some(last)) = (&text, last) {
            let mut l = *first;
            while l <= last {
                self.rows
                    .push(row_of(term, scrolled, l).map(|(cells, wrap)| {
                        self.cells.extend_from_slice(cells);
                        (cells.len(), wrap)
                    }));
                l = l.below();
            }
        }
        self.text = text;
    }

    /// Whether every line it covers holds now what it held.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn kept(&self, term: &Term<'_>, scrolled: Scrolled, first: AbsLine) -> bool {
        let mut l = first;
        let mut from = 0;
        for row in &self.rows {
            let now = row_of(term, scrolled, l);
            let same = match (row, now) {
                (None, None) => true,
                (Some((n, wrap)), Some((cells, w))) => {
                    *wrap == w && self.cells.get(from..from + n) == Some(cells)
                }
                _ => false,
            };
            if !same {
                return false;
            }
            from += row.map_or(0, |(n, _)| n);
            l = l.below();
        }
        true
    }
}

/// **The link preview's state**: the cell under the pointer, whatever the
/// modifiers, and what it draws. A driver updates it on each pointer
/// event and modifier change, refreshes it while the pointer is over the
/// text -- on Linux at each frame, on macOS on a turn that owes it a read
/// ([`HoverOwed`], [`Hover::under_moved`]) -- owing a frame when it
/// changes, and clears it when the window loses the keyboard. Every read builds its [`Spot`] from the view shown then, as
/// a click at the same place would: a window held on its line while
/// output retires lines, or moved by a wheel, shows other lines under a
/// pointer that has not moved. A read repeated over what has not changed
/// reads no text ([`ReadOf`]), since with [`AtRest::Look`] it runs with no
/// modifier, at every frame the pointer rests over the text on Linux.
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
    /// What the last read went over, as it then was.
    covered: Covered,
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
        host: &MachineNames,
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
        host: &MachineNames,
        paths: &dyn PathsAt,
    ) -> bool {
        self.at = at;
        let inputs = (at, scrolled, chord, focus);
        let generation = (paths.generation(), paths.origins());
        if self
            .read
            .as_ref()
            .is_some_and(|r| r.matches(term, inputs, host, generation))
        {
            return false;
        }
        let mut kept = self
            .read
            .take()
            .map_or_else(MachineNames::default, |r| r.host);
        kept.clone_from(host);
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
        let mut text = Text::Unread;
        self.buffers.visited = None;
        let found = at
            .filter(|_| previewing || paint.is_some())
            .and_then(|(row, col)| {
                let s = spot_now(term, scrolled, row, col)?;
                self.reads.bump();
                let line = s.scrolled.line(s.back, s.row);
                text = Text::Lines {
                    first: line,
                    line,
                    rendition: term.view(s.back).rendition(s.row),
                    clusters: term.cluster_epoch(),
                };
                let link = hovered_in(term, s, &mut self.buffers, paths)?;
                if matches!(link, Under::Osc8(_)) {
                    text = Text::Slot;
                }
                let printed = paths.printed(term, &link);
                let opens = matches!(
                    verdict(&link, term, host, paths, printed),
                    Some((_, Ok(())))
                );
                // Gone: under the chord its cells are struck, whether or not
                // they are at rest, and the bar says why; nothing is struck
                // while `missing` is off.
                if let Some(g) =
                    gone_of(&link, term, host, paths, printed).filter(|_| paths.strikes())
                {
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
        let (text, last) = match text {
            Text::Lines {
                line,
                rendition,
                clusters,
                ..
            } => {
                let (lo, hi) = self.buffers.visited.unwrap_or((line, line));
                let (lo, hi) = (lo.min(line), hi.max(line));
                let first = lo.above().unwrap_or(lo);
                let text = Text::Lines {
                    first,
                    line,
                    rendition,
                    clusters,
                };
                (text, Some(hi.below()))
            }
            other => (other, None),
        };
        self.covered.take(term, scrolled, text, last);
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
        host: &MachineNames,
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
        host: &MachineNames,
        paths: &dyn PathsAt,
    ) -> bool {
        match self.at {
            Some(at) => self.update_with(term, Some(at), scrolled, chord, focus, host, paths),
            None => false,
        }
    }

    /// **Whether what the last read went over moved**, so the link under a
    /// resting pointer is owed a read ([`HoverOwed`]): the window moved under
    /// the pointer onto another line, the grid resized, a line it read
    /// written, wrapped or sized again, or what names its links, its
    /// clusters, this machine or the paths' answers moved. Off the text, or
    /// where no text was read, only an event or a resize can change what a
    /// read finds. It reads no text: each covered line is compared with its
    /// copy (`Covered`), a few rows of cells, where a read would walk the
    /// line, find its link and judge it. Under an `OSC 8` link, a byte
    /// taken or a window moved owes the read, since the link's cells are any
    /// on the view naming its slot.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn under_moved(
        &self,
        term: &Term<'_>,
        scrolled: Scrolled,
        host: &MachineNames,
        paths: &dyn PathsAt,
    ) -> HoverOwed {
        let Some((row, _)) = self.at else {
            return HoverOwed::No;
        };
        let Some(read) = &self.read else {
            return HoverOwed::Yes;
        };
        let back = term.window_back();
        let kept = read.geometry == term.geometry()
            && match &self.covered.text {
                Text::Unread => true,
                Text::Slot => {
                    read.bytes == Taken(term.amplification().0)
                        && read.back == back
                        && read.scrolled == scrolled
                        && read.epoch == term.link_epoch()
                        && read.host == *host
                        && read.paths == (paths.generation(), paths.origins())
                }
                Text::Lines {
                    first,
                    line,
                    rendition,
                    clusters,
                } => {
                    read.epoch == term.link_epoch()
                        && read.host == *host
                        && read.paths == (paths.generation(), paths.origins())
                        && *clusters == term.cluster_epoch()
                        && scrolled.line(back, row) == *line
                        && term.view(back).rendition(row) == *rendition
                        && self.covered.kept(term, scrolled, *first)
                }
            };
        match kept {
            true => HoverOwed::No,
            false => HoverOwed::Yes,
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

    const NONE: &MachineNames = &MachineNames::NONE;

    /// **This machine's names match a host without case, and an empty name
    /// is none**: `Mac.local` matches `mac.LOCAL`; an empty name given is
    /// left out, so it matches nothing; none known matches no name.
    /// Named break: compare with case -- `mac.LOCAL` is another machine.
    #[test]
    fn machine_names_match_without_case_and_hold_no_empty_name() {
        let names = MachineNames::of(["".to_owned(), "Mac.local".to_owned()]);
        assert!(names.is_this(b"mac.LOCAL"));
        assert!(!names.is_this(b""), "the empty name left out");
        assert!(!NONE.is_this(b"mac.local"));
        assert_eq!(MachineNames::one(""), MachineNames::NONE);
    }

    /// **One set of this machine's names answers the policy and the
    /// paths**: with `gethostname`'s name and the process info's
    /// `hostName` differing, as on a Mac, a `file://` link naming either is
    /// admitted by the policy a click and a line's look go by, and taken as
    /// this machine's by `file_path`, which the struck look reads. The
    /// names are given, not read from this machine. Named break: hand the
    /// policy `hostName` alone (`hand_names`) -- the link naming
    /// `gethostname`'s name is refused by the policy while the paths take
    /// it.
    #[test]
    fn the_policy_and_the_paths_take_the_same_names() {
        let mut uris = crate::lined::Uris::default();
        let mut paths = crate::paths::Paths::new(None, crate::paths::Forms::DEFAULT);
        let names = MachineNames::of(["mac".to_owned(), "mac.local".to_owned()]);
        hand_names(names, &mut uris, &mut paths);
        for uri in ["file://mac/etc/hosts", "file://mac.local/etc/hosts"] {
            assert!(admit(uri.as_bytes(), uris.names()).is_ok(), "{uri}: policy");
            assert!(
                crate::paths::file_path(uri.as_bytes(), paths.names()).is_some(),
                "{uri}: paths"
            );
        }
        assert!(admit(b"file://other/etc/hosts", uris.names()).is_err());
    }

    /// **The names reach the links and the paths only through
    /// `hand_names`, and the window hands every name it read**: in the
    /// app's own code (comments, literals and tests left out), every
    /// `.set_names(` call is one of `hand_names`' two, so no driver hands
    /// either side a set of its own; and the macOS window's
    /// `link_host_check` hands all the names its thread read, through
    /// `hand_host_names`, as the opening hands `gethostname`'s.
    /// Named breaks: a `set_names` call outside `hand_names` (in
    /// `hand_host_names`) -- the first part names its file; the window
    /// handing its last name alone (`MachineNames::of(names.last()..)`) --
    /// the second part fails.
    #[test]
    fn the_names_are_handed_once_and_whole() {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, usize)>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .expect("the sources")
                .flatten()
                .map(|e| e.path())
                .collect();
            entries.sort();
            for path in entries {
                let name = path.to_string_lossy().into_owned();
                if path.is_dir() {
                    walk(&path, out);
                } else if name.ends_with(".rs") && !name.ends_with("tests.rs") {
                    let text = std::fs::read_to_string(&path).expect("a source");
                    let code = crate::no_appkit::without_tests(&crate::no_appkit::bare_code(&text));
                    let n = code.matches(".set_names(").count();
                    if n > 0 {
                        out.push((name, n));
                    }
                }
            }
        }
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found = Vec::new();
        walk(&src, &mut found);
        let found: Vec<(String, usize)> = found
            .into_iter()
            .map(|(p, n)| (p.trim_start_matches(&*src.to_string_lossy()).to_owned(), n))
            .collect();
        assert_eq!(
            found,
            [("/link.rs".to_owned(), 2)],
            "set_names outside hand_names"
        );
        let link = crate::no_appkit::bare_code(include_str!("link.rs"));
        let hand = crate::no_appkit::fn_body(&link, concat!("pub fn hand_", "names("));
        assert_eq!(
            hand.matches(".set_names(").count(),
            2,
            "both inside hand_names"
        );
        let app = include_str!("app.rs");
        let check = crate::no_appkit::fn_body(app, concat!("fn link_host_", "check(&mut self) {"));
        assert!(
            check.contains(concat!("self.hosts.", "owed()"))
                && check.contains(concat!("self.hand_host_", "names(names)")),
            "the window hands every name it read"
        );
        let hand = crate::no_appkit::fn_body(
            app,
            concat!("fn hand_host_", "names(&mut self, names: Vec<String>) {"),
        );
        assert!(
            hand.contains("MachineNames::of(names)") && hand.contains("hand_names(names, "),
            "whole, through hand_names"
        );
    }

    /// **DT-L3: a `file:` URI is RFC 8089's, read once**: `file:///etc/x?q#f`,
    /// a query or a fragment alone, `file:///a%ZZ` and `file:////srv/x` (a
    /// UNC host written as a path) are off the grammar and refused, as the
    /// printed paths' reader refuses them; another machine's is refused as
    /// one; the controls, `file:///etc/hosts` and the local-path form
    /// `file:/etc/hosts`, are admitted. Named break: admit a `file:` URI by
    /// its host alone -- the query, the fragment, the broken escape and the
    /// UNC form are admitted.
    #[test]
    fn a_file_uri_is_rfc_8089s() {
        for uri in ["file:///etc/hosts", "file:/etc/hosts"] {
            assert!(admit(uri.as_bytes(), NONE).is_ok(), "the control: {uri}");
        }
        for uri in [
            "file:///etc/x?q#f",
            "file:///etc/x?q",
            "file:///etc/x#f",
            "file:///a%ZZ",
            "file:////srv/x",
        ] {
            assert_eq!(
                admit(uri.as_bytes(), NONE).err(),
                Some(Refused::Syntax),
                "{uri}"
            );
        }
        assert_eq!(
            admit(
                b"file://elsewhere/x",
                &crate::link::MachineNames::one("here")
            )
            .err(),
            Some(Refused::Host)
        );
    }

    /// **DT-L4: an `http` URI with an empty host is refused** (RFC 9110
    /// § 4.2.1): the host is after the last `@` and before the port, an
    /// IP-literal's brackets kept whole, so `http://@/x`, `http://:80/`,
    /// `https://u@/` and `http://[]/` are refused, and so is an IP-literal
    /// left open; the controls, with userinfo, a port and `[::1]`, are
    /// admitted. Named break: take the host as everything before the path,
    /// userinfo and port included -- the first three are admitted.
    #[test]
    fn an_http_uri_with_an_empty_host_is_refused() {
        for uri in [
            "http://h/x",
            "https://u@h:443/",
            "http://[::1]:80/",
            "http://u:p@h",
        ] {
            assert!(admit(uri.as_bytes(), NONE).is_ok(), "the control: {uri}");
        }
        for uri in [
            "http://@/x",
            "http://:80/",
            "https://u@/",
            "http://[]/",
            "http://[::1/",
        ] {
            assert_eq!(
                admit(uri.as_bytes(), NONE).err(),
                Some(Refused::Host),
                "{uri}"
            );
        }
    }

    /// **DT-L4: a `mailto:` link is handed on as it was parsed** (RFC
    /// 6068 § 2): rebuilt from its recipients and its headers, each value
    /// percent-decoded and encoded again, so `subject=x?bcc=e@x` is handed
    /// over as one subject, `x%3Fbcc%3De%40x`, never a hidden recipient; a
    /// value already encoded keeps its bytes, and a `#` is escaped as
    /// `%23`. A recipient in a `to` header alone is a recipient; no
    /// recipient at all, or a header other than `to`, `subject` and `body`,
    /// is refused. Named break: hand over the URI as printed -- the
    /// subject's `?bcc=` reaches the mail client raw.
    #[test]
    fn a_mailto_link_is_handed_on_as_it_was_parsed() {
        let handed = |uri: &str| admit(uri.as_bytes(), NONE).map(|t| t.handed().into_owned());
        assert_eq!(
            handed("mailto:a@b?subject=x?bcc=e@x"),
            Ok("mailto:a@b?subject=x%3Fbcc%3De%40x".to_owned())
        );
        assert_eq!(
            admit(b"mailto:a@b?subject=x?bcc=e@x", NONE).map(|t| t.to_admitted().into_string()),
            Ok("mailto:a@b?subject=x%3Fbcc%3De%40x".to_owned()),
            "what the opener gets"
        );
        assert_eq!(
            handed("mailto:a@b?Subject=caf%C3%A9&body=a%20b#c"),
            Ok("mailto:a@b?subject=caf%C3%A9&body=a%20b%23c".to_owned())
        );
        assert_eq!(handed("mailto:a@b"), Ok("mailto:a@b".to_owned()));
        assert_eq!(handed("mailto:?to=a@b"), Ok("mailto:?to=a%40b".to_owned()));
        assert_eq!(handed("mailto:"), Err(Refused::Host));
        assert_eq!(handed("mailto:?subject=x"), Err(Refused::Host));
        assert_eq!(handed("mailto:a@b?bcc=e@x"), Err(Refused::Header));
        assert_eq!(
            handed("https://h/?a=b").as_deref(),
            Ok("https://h/?a=b"),
            "another scheme as admitted"
        );
    }

    /// **M1: a line break decoded into a mail header is refused** (RFC
    /// 6068 § 5): a `subject` or `to` value, or the recipients, decoding to
    /// a control -- `%0D%0A` above all, which would end the header and
    /// begin a `Bcc:` of its own -- is refused; the body keeps `%0D%0A`,
    /// the line break it is to carry. Named break: decode without the
    /// check -- the subject's and the recipients' line breaks are admitted.
    #[test]
    fn a_line_break_decoded_into_a_mail_header_is_refused() {
        for uri in [
            "mailto:a@b?subject=x%0D%0ABcc:%20e@x",
            "mailto:a%0D%0Ab@x",
            "mailto:?to=a@b%0ABcc:e@x",
            "mailto:a@b?subject=%7F",
        ] {
            assert_eq!(
                admit(uri.as_bytes(), NONE).err(),
                Some(Refused::Header),
                "{uri}"
            );
        }
        let body = admit(b"mailto:a@b?body=a%0D%0Ab", NONE).map(|t| t.handed().into_owned());
        assert_eq!(
            body,
            Ok("mailto:a@b?body=a%0D%0Ab".to_owned()),
            "the body's line break"
        );
    }

    /// **A `file:` link carries the file the policy read, and a reveal
    /// names it with no host**: the path decoded from what was admitted,
    /// and the URI the file manager is handed has this machine's name and
    /// `localhost` left out; a URI of another scheme is handed as admitted.
    /// Named break: hand the file manager the URI as admitted -- the first
    /// reveal keeps the host.
    #[test]
    fn a_file_link_carries_its_file_and_reveals_it_with_no_host() {
        let here = &crate::link::MachineNames::one("thishost.local");
        let t = admit(b"file://thishost.local/tmp/a%20b", here).expect("admitted");
        assert_eq!(
            t.local_file().and_then(|f| f.path()),
            Some(b"/tmp/a b".to_vec())
        );
        assert_eq!(t.reveal_uri().as_str(), "file:///tmp/a%20b");
        let t = admit(b"file://localhost/p", NONE).expect("admitted");
        assert_eq!(t.reveal_uri().as_str(), "file:///p");
        let t = admit(b"https://example.invalid/", NONE).expect("admitted");
        assert_eq!(t.local_file(), None);
        assert_eq!(t.reveal_uri().as_str(), "https://example.invalid/");
    }

    /// **`admit_owned` decides as `admit` does and keeps the very bytes it
    /// checked**, and `AdmittedUri::checked` is called in this file alone,
    /// after the checks: its door is the policy's: the same action, the same refusal, and the `String`
    /// moved out is the one moved in -- its buffer's address unchanged, so
    /// nothing was copied. Perturbation: rebuild the URI from the checked
    /// `&str` and the address assertion fails.
    #[test]
    fn admit_owned_moves_the_uri_it_checked() {
        let uri = String::from("file:///tmp/a%20b");
        let at = uri.as_ptr();
        let t = admit_owned(uri, NONE).expect("admitted");
        assert_eq!((t.action(), t.scheme()), (Action::Reveal, Scheme::File));
        let back = t.into_uri().into_string();
        assert_eq!(back, "file:///tmp/a%20b");
        assert_eq!(back.as_ptr(), at, "moved, not copied");
        assert_eq!(
            admit_owned(String::from("javascript:x"), NONE).map(|t| t.action()),
            admit(b"javascript:x", NONE).map(|t| t.action()),
        );
    }

    /// **Only the admission mints an `AdmittedUri`**, in every crate of the
    /// workspace: the type is `pub` in baud-domain, so a door anywhere else
    /// (baud-present-macos above all, which hands the URI to the OS) would
    /// let an unchecked URI through. Every `.rs` under a `src` directory of
    /// the repository is read; tests and benches outside `src` may mint one.
    /// The control is link.rs's own three: the owned admission, a borrowed
    /// target's copy and its reveal's. Perturbation: call
    /// `AdmittedUri::checked` in baud-present-macos/src/link.rs and this
    /// names it.
    #[test]
    fn no_crate_but_the_admission_mints_an_admitted_uri() {
        /// Whether a directory is under a crate's `src`.
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Within {
            Src,
            Outside,
        }
        fn walk(dir: &std::path::Path, within: Within, out: &mut Vec<(std::path::PathBuf, usize)>) {
            let door = concat!("AdmittedUri::", "checked(");
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if path.is_dir() {
                    if matches!(&*name, "target" | ".git" | ".devenv" | ".claude" | "docs") {
                        continue;
                    }
                    let within = match (within, &*name) {
                        (Within::Src, _) | (_, "src") => Within::Src,
                        _ => Within::Outside,
                    };
                    walk(&path, within, out);
                } else if within == Within::Src && name.ends_with(".rs") {
                    let n = std::fs::read_to_string(&path).map_or(0, |s| s.matches(door).count());
                    if n > 0 {
                        out.push((path, n));
                    }
                }
            }
        }
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the workspace");
        let mut found = Vec::new();
        walk(repo, Within::Outside, &mut found);
        let found: Vec<_> = found
            .iter()
            .map(|(p, n)| (p.strip_prefix(repo).unwrap_or(p).to_path_buf(), *n))
            .collect();
        assert_eq!(
            found,
            [(std::path::PathBuf::from("baud-app/src/link.rs"), 3)],
            "only the admission mints one"
        );
    }

    fn ok(uri: &str) -> Target<'_> {
        admit(
            uri.as_bytes(),
            &crate::link::MachineNames::one("thishost.local"),
        )
        .unwrap_or_else(|r| panic!("{uri}: {r:?}"))
    }

    fn no(uri: &str) -> Refused {
        admit(
            uri.as_bytes(),
            &crate::link::MachineNames::one("thishost.local"),
        )
        .expect_err(uri)
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
        assert!(
            admit(
                b"file://THISHOST.local/p",
                &crate::link::MachineNames::one("thishost.local")
            )
            .is_ok()
        );
        assert_eq!(
            admit(
                b"file://other/p",
                &crate::link::MachineNames::one("thishost.local")
            ),
            Err(Refused::Host)
        );
        assert_eq!(admit(b"file://thishost.local/p", NONE), Err(Refused::Host));
        // No path is off RFC 8089's grammar; the local-path form is on it.
        assert_eq!(no("file://p"), Refused::Syntax);
        assert!(admit(b"file:/p", NONE).is_ok());
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
            view: (Scrolled::START, Rows::ZERO),
        })
    }

    #[allow(non_snake_case)]
    fn M() -> Under {
        Under::Osc8(Osc8 {
            id: LinkId::from_index(4),
            epoch: LinkEpoch::ZERO,
            view: (Scrolled::START, Rows::ZERO),
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
            joined: Vec::new(),
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
        click_with(t, down, up, &NoPaths)
    }

    /// [`click`], read through `paths`.
    fn click_with(t: &Term<'_>, down: Spot, up: Spot, paths: &dyn PathsAt) -> Gesture {
        let under_down = under_with(t, down, Press::Down, Chord::Cmd, Focus::Gained, None, paths);
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
        let under_up = under_with(
            t,
            up,
            Press::Up,
            Chord::Other,
            Focus::Gained,
            Some(&armed),
            paths,
        );
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
        let target = admit(b.uri().unwrap(), &crate::link::MachineNames::NONE).expect("admitted");
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

    /// **A hard line break ends the URI where the row ends short of the last
    /// column**, or where `[link] join_rows` is off: the next row is not
    /// joined. Perturbation: ignore `[link] join_rows` in `Paths::joins` --
    /// the second case reads `bcd` on.
    #[test]
    fn a_hard_line_break_ends_the_uri() {
        let s = Scrolled::START;
        let at = spot(s, Row::from_index(0), Col::at_index(9));
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"https://example.c/a\r\nbcd");
        assert_eq!(
            opened(&click(&t, at, at)).as_deref(),
            Some(&b"https://example.c/a"[..])
        );
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"https://example.co/a\r\nbcd");
        let mut apart = crate::paths::Paths::default();
        let _ = apart.set_join(crate::paths::Turned::Off);
        assert_eq!(
            opened(&click_with(&t, at, at, &apart)).as_deref(),
            Some(&b"https://example.co/a"[..]),
            "join_rows off"
        );
    }

    /// **A row the writer ended at the last column, the next starting in a
    /// URI's characters, is read on**: a click on either row opens the whole
    /// address, as a multiplexer repainting a full-width pane a row at a time
    /// leaves it -- a cursor move per row, or CR LF. Perturbation: drop the
    /// join from `read_window`'s walks -- the clicks open the first row's part
    /// or nothing.
    #[test]
    fn a_row_the_writer_ended_at_the_last_column_is_read_on() {
        let s = Scrolled::START;
        for ends in [&b"\x1b[2;1H"[..], b"\r\n"] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut bytes = b"https://example.co/a".to_vec();
            bytes.extend_from_slice(ends);
            bytes.extend_from_slice(b"bcd ok");
            feed(&mut t, &bytes);
            for (row, col) in [(0, 9), (1, 1)] {
                let at = spot(s, Row::from_index(row), Col::at_index(col));
                assert_eq!(
                    opened(&click(&t, at, at)).as_deref(),
                    Some(&b"https://example.co/abcd"[..]),
                    "{ends:?} at ({row}, {col})"
                );
            }
        }
    }

    /// **What a join reads is kept only where it crosses the join and
    /// admits**: a URI starting the next row, after a word reaching the last
    /// column, is read as the next row's own; and a URI the join would make
    /// refused opens as it did unjoined, its cut part. Perturbation: keep
    /// whatever the joined read finds in `read_under`, crossing or not and
    /// refused or not -- red.
    #[test]
    fn a_join_that_does_not_cross_or_admit_reads_as_unjoined() {
        let s = Scrolled::START;
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"twenty-letters-words\r\nhttps://e.example/x");
        let at = spot(s, Row::from_index(1), Col::at_index(10));
        assert_eq!(
            opened(&click(&t, at, at)).as_deref(),
            Some(&b"https://e.example/x"[..])
        );
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"https://example.co/a\r\n%zz");
        let at = spot(s, Row::from_index(0), Col::at_index(9));
        assert_eq!(
            opened(&click(&t, at, at)).as_deref(),
            Some(&b"https://example.co/a"[..])
        );
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
        let text = |col, room| {
            preview_text(
                &link(col),
                &t,
                &crate::link::MachineNames::NONE,
                Cols::of_index(room),
            )
            .unwrap()
        };
        assert_eq!(
            text(0, 80),
            "Won't open, not a web, mail or file link  ftp://f.example/x"
        );
        assert_eq!(text(4, 80), "https://h.example/", "the control: it opens");
        assert_eq!(text(0, 30), "Won't open  ftp://f.example/x", "a narrow row");
    }

    /// **The hover shows a `mailto:` link as it is handed on**: a link
    /// whose subject spells `?bcc=` shows the subject rebuilt, the `?`, the
    /// `=` and the `@` in it escaped, as the mail client gets it. Named
    /// break: show the address as printed -- the hidden `bcc` shows raw.
    #[test]
    fn the_hover_shows_a_mailto_link_as_it_is_handed_on() {
        let mut owned = Owned::new(&WinSize {
            ws_col: 80,
            ws_row: 6,
        });
        let mut t = Term::new(
            owned.storage(),
            &WinSize {
                ws_col: 80,
                ws_row: 6,
            },
        );
        feed(
            &mut t,
            b"\x1b]8;;mailto:a@b?subject=x?bcc=e@x\x1b\\mail\x1b]8;;\x1b\\",
        );
        let link = hovered(&t, spot(Scrolled::START, Row::ZERO, Col::ZERO)).unwrap();
        let text = preview_text(
            &link,
            &t,
            &crate::link::MachineNames::NONE,
            Cols::of_index(80),
        );
        assert_eq!(text.as_deref(), Some("mailto:a@b?subject=x%3Fbcc%3De%40x"));
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

    /// **Output owes the hover a read only where its last read went**
    /// ([`Hover::under_moved`]). The look at rest over a URI a soft wrap
    /// carries from the first row onto the third: a write to the last row
    /// owes nothing; a write to the third row, which the read reached two
    /// rows from the pointer, owes one, as do a write to the fourth, which
    /// the URI's run-on test reads, and to the pointer's own row, and
    /// another line under the pointer. Off the text, or with no look and no
    /// chord, nothing is owed by any write. Over an `OSC 8` link any byte
    /// owes one, since its cells are any naming its slot. Perturbations:
    /// the copy kept of the pointer's line alone -- the third row's write
    /// owes nothing; the line past the read's end left out -- the fourth's
    /// owes nothing; the compare dropped -- no write owes one.
    #[test]
    fn output_owes_the_hover_a_read_only_where_its_last_read_went() {
        use baud_render::HoverPaint;
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        // Fifty bytes on twenty columns: two rows and ten cells of the third.
        feed(
            &mut t,
            b"https://a.example/abcdefghijklmnopqrstuvwxyz012345",
        );
        let (at, s, key) = (
            Some((Row::ZERO, Col::at_index(2))),
            Scrolled::START,
            Focus::Gained,
        );
        let mut h = Hover::default();
        h.set_at_rest(AtRest::Look(HoverPaint::Box));
        let _ = h.update(&t, at, s, Chord::Other, key, NONE);
        assert!(h.over().is_some(), "the URI is drawn at rest");
        let moved = |h: &Hover, t: &Term<'_>| h.under_moved(t, s, NONE, &NoPaths);
        assert_eq!(moved(&h, &t), HoverOwed::No, "nothing written yet");
        feed(&mut t, b"\x1b[6;1Haway");
        assert_eq!(moved(&h, &t), HoverOwed::No, "the last row is not read");
        for (row, what) in [
            (3, "the row the wrap carried it onto"),
            (4, "the row past the read's end"),
            (1, "the pointer's own row"),
        ] {
            feed(&mut t, format!("\x1b[{row};19Hz").as_bytes());
            assert_eq!(moved(&h, &t), HoverOwed::Yes, "{what}");
            let _ = h.refresh(&t, s, Chord::Other, key, NONE);
            assert_eq!(moved(&h, &t), HoverOwed::No, "read again: {what}");
        }
        let later = s.advanced(Rows::of_count(1));
        assert_eq!(
            h.under_moved(&t, later, NONE, &NoPaths),
            HoverOwed::Yes,
            "another line under the pointer"
        );
        // Off the text, and with no look and no chord, no write owes one.
        let _ = h.update(&t, None, s, Chord::Other, key, NONE);
        feed(&mut t, b"\x1b[1;1Hx");
        assert_eq!(moved(&h, &t), HoverOwed::No, "off the text");
        h.set_at_rest(AtRest::Off);
        let _ = h.update(&t, at, s, Chord::Other, key, NONE);
        feed(&mut t, b"\x1b[1;1Hy");
        assert_eq!(moved(&h, &t), HoverOwed::No, "nothing to read");
        // An `OSC 8` link: a byte anywhere owes the read.
        h.set_at_rest(AtRest::Look(HoverPaint::Box));
        feed(
            &mut t,
            b"\x1b[1;1H\x1b]8;;https://b.example/\x1b\\link\x1b]8;;\x1b\\",
        );
        let _ = h.update(&t, at, s, Chord::Other, key, NONE);
        assert!(h.over().is_some(), "the link is drawn at rest");
        assert_eq!(moved(&h, &t), HoverOwed::No);
        feed(&mut t, b"\x1b[5;1Hb");
        assert_eq!(moved(&h, &t), HoverOwed::Yes, "a byte under a slot");
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

    /// **A program's `OSC 22` shape reaches the pointer with no read**: the
    /// shape over the text is [`Hover::pointer`]'s answer from the look the
    /// last read left and the shape asked, which the driver asks again after
    /// each read it feeds (`reshape_pointer`). So a new shape over plain text
    /// at rest is the pointer's at once, the hover's read count unchanged,
    /// and the shape written owes no read ([`Hover::under_moved`]): it moves
    /// no cell the read went over. Perturbation: a read owed by any byte
    /// taken -- the last assertion fails.
    #[test]
    fn a_programs_pointer_shape_needs_no_read_of_the_link() {
        use baud_render::HoverPaint;
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"plain words");
        let (at, s, key) = (
            Some((Row::ZERO, Col::at_index(2))),
            Scrolled::START,
            Focus::Gained,
        );
        let mut h = Hover::default();
        h.set_at_rest(AtRest::Look(HoverPaint::Box));
        let _ = h.update(&t, at, s, Chord::Other, key, NONE);
        let reads = h.reads();
        assert_eq!(
            h.pointer(t.pointer_shape()),
            None,
            "the control: none asked"
        );
        feed(&mut t, b"\x1b]22;crosshair\x1b\\");
        assert_eq!(
            h.pointer(t.pointer_shape()),
            Some(PointerShape::Crosshair),
            "the shape asked, at once"
        );
        assert_eq!(h.reads(), reads, "and no read for it");
        assert_eq!(h.under_moved(&t, s, NONE, &NoPaths), HoverOwed::No);
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

        fn printed(&self, term: &Term<'_>, link: &Under) -> Printed {
            NoPaths.printed(term, link)
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

    /// The text of the arm starting at `head`, through the line that closes
    /// it at `head`'s own indent, a `}` or a match arm's `},`.
    fn arm<'a>(src: &'a str, head: &str) -> &'a str {
        let at = src.find(head).unwrap_or_else(|| panic!("no {head:?}"));
        let line = src[..at].rfind('\n').map_or(0, |n| n + 1);
        let indent = &src[line..at];
        let end = [format!("\n{indent}}}\n"), format!("\n{indent}}},\n")]
            .iter()
            .filter_map(|close| src[at..].find(close.as_str()))
            .min()
            .unwrap_or_else(|| panic!("{head:?} never closes"));
        &src[at..at + end]
    }

    /// **A click on a window rule is refused on a driver until that
    /// driver's own commit opens the window**, and both drivers now do: each
    /// one's link arm and printed-path arm ask for the window, and neither
    /// reaches a reveal, the printed path's `if let` ending in its own
    /// return. Named break: drop the ask on either driver. Without the
    /// return the arm no longer compiles here, since it moves the click.
    #[test]
    fn a_window_click_is_refused_until_its_driver_opens_it() {
        // The app's link arm is the shared decision's window
        // (`link_layer::decide`).
        let drivers = [
            (
                "app.rs",
                include_str!("app.rs"),
                "OsEffect::OpenWindow",
                "Then::Window(built) => match built {",
            ),
            (
                "linux.rs",
                include_str!("linux.rs"),
                "Later::Window(",
                "Click::Window => {",
            ),
        ];
        assert!(
            include_str!("link_layer.rs").contains(concat!(
                "Then::Window(t.rule()",
                ".and_then(|ix| uris.rules().window(ix, &t.handed())))"
            )),
            "the shared decision's window"
        );
        for (name, src, ask, link) in drivers {
            for head in [link, "if let Found::Window("] {
                let body = arm(src, head);
                assert!(body.contains(ask), "{name} {head}: no {ask}");
                assert!(!body.contains("Reveal"), "{name} {head}: a reveal");
                let ends = head.starts_with("if let") && !body.contains("return;");
                assert!(!ends, "{name} {head}: falls through");
            }
        }
    }
    /// **No door lets a URI through without saying where it came from**:
    /// `admit_with`, the policy with no origin, is private to this module,
    /// behind `admit_link` (a URI read off the screen, with its origin),
    /// `admit_path` (a printed path baud found here) and `admit_about` (an
    /// address baud ships); the bare `admit` is the policy's own tests'.
    /// baud-app is a binary crate, where no `compile_fail` runs, so the
    /// source is read, with the private definition as its positive
    /// control. Named break: make `admit_with` public again.
    #[test]
    fn admit_with_is_reached_only_through_a_named_door() {
        let src = include_str!("link.rs");
        let (code, _) = src
            .split_once("\n#[cfg(test)]\nmod tests")
            .expect("the tests");
        assert!(
            code.contains(concat!("\nfn admit_", "with<'a>(")),
            "the control: defined here, private"
        );
        for open in ["pub ", "pub(crate) ", "pub(super) "] {
            assert!(
                !code.contains(&format!("{open}fn admit_{}", "with")),
                "{open}admit_with"
            );
        }
        assert!(
            code.contains(concat!("#[cfg(test)]\npub fn admit", "<'a>(")),
            "the bare policy is the tests'"
        );
        for door in ["admit_link", "admit_path", "admit_about"] {
            assert!(code.contains(&format!("pub fn {door}")), "{door}");
        }
    }
}
