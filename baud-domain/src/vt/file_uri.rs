//! **`OSC 7 ; Pt`: a shell reporting its working directory, as a `file:`
//! URI.** No standard defines the sequence: the terminal-wg issue asking for
//! one says "to my knowledge there is no documentation on the behavior of
//! this escape" (`[[source: terminal-wg-issue-20]]`), and Apple's own shell
//! files send it "using a "file:" scheme URL, including the host name to
//! disambiguate local vs. remote paths ... Percent-encode the pathname"
//! (`[[source: macos-zshrc-apple-terminal]]` 17-20). **What the payload is
//! does have a standard**, and it is read here exactly as written:
//! `[[source: rfc8089]]` § 2 (175-187),
//!
//! ```text
//! file-URI       = file-scheme ":" file-hier-part
//! file-hier-part = ( "//" auth-path ) / local-path
//! auth-path      = [ file-auth ] path-absolute
//! local-path     = path-absolute
//! file-auth      = "localhost" / host
//! ```
//!
//! with `host` and `path-absolute` from `[[source: rfc3986]]` (2722-2787):
//! `path-absolute = "/" [ segment-nz *( "/" segment ) ]`, `pchar =
//! unreserved / pct-encoded / sub-delims / ":" / "@"`, `reg-name = *(
//! unreserved / pct-encoded / sub-delims )`. The grammar has no query and no
//! fragment, so a `?` or a `#` is off it. A byte the grammar does not allow
//! -- a space, a raw UTF-8 byte, a control -- makes the report malformed:
//! RFC 8089 § 4 asks for a name's UTF-8 "before percent-encoding is applied"
//! (253-257), and an unencoded path is not a URI. A shell integration that
//! sends one unencoded (`kitty-shell-cwd://`) is refused and counted by the
//! core, which then reads the directory the way it reads one that sent
//! nothing.
//!
//! **The scheme and `localhost` are matched without case**: ABNF's quoted
//! strings are case-insensitive, and RFC 3986 § 3.1 says "An implementation
//! should accept uppercase letters as equivalent to lowercase in scheme
//! names" (915-917). **The check is a table**, one class per byte built at
//! compile time, so the walk does a lookup a byte and nothing else.

/// What an `OSC 7` body reports.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DirectoryReport<'a> {
    /// A `file:` URI on RFC 8089's grammar.
    File(FileUri<'a>),
    /// A URI of another scheme: `kitty-shell-cwd://` is the one in use.
    OtherScheme,
    /// Not a URI on the grammar: empty, no scheme, a byte the grammar does
    /// not allow, a broken `%` escape, a query or a fragment, no path.
    Malformed,
}

/// The host a file URI names. **An empty `file-auth` and `localhost` both
/// mean this machine**: "the special string "localhost" that is interpreted
/// as "the machine from which the URI is being interpreted," exactly as if
/// no authority were present" (RFC 8089 § 2, 194-197). Any other name is
/// left for the embedder to compare with its own: "a fully qualified domain
/// name that resolves to the machine from which the URI is being
/// interpreted" (§ 3, 236-240) is a question for whoever knows the machine,
/// and a no_std core does not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileHost<'a> {
    Local,
    Named(&'a [u8]),
}

/// A file URI's host and its path, the path still percent-encoded: every
/// byte of it is on the grammar, so [`FileUri::decode_path`] cannot meet a
/// broken escape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileUri<'a> {
    pub host: FileHost<'a>,
    path: &'a [u8],
}

/// A byte's place in the grammar: unreserved or a sub-delim (`reg-name` and
/// `pchar` both), the two more `pchar` takes (`:` and `@`), `%` beginning an
/// escape, `/`, a hexadecimal digit's value, or nothing the grammar has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Name,
    PathOnly,
    Percent,
    Slash,
    Off,
}

const fn class_of(b: u8) -> Class {
    match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => Class::Name,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' => Class::Name,
        b':' | b'@' => Class::PathOnly,
        b'%' => Class::Percent,
        b'/' => Class::Slash,
        _ => Class::Off,
    }
}

const fn build_classes() -> [Class; 256] {
    let mut t = [Class::Off; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = class_of(i as u8);
        i += 1;
    }
    t
}

/// Every byte's [`Class`], built at compile time.
const CLASSES: [Class; 256] = build_classes();

/// A hexadecimal digit's value, `NOT_HEX` for any other byte.
const NOT_HEX: u8 = 0xff;

const fn build_hex() -> [u8; 256] {
    let mut t = [NOT_HEX; 256];
    let mut i = 0;
    while i < 10 {
        t[b'0' as usize + i] = i as u8;
        i += 1;
    }
    let mut i = 0;
    while i < 6 {
        t[b'a' as usize + i] = 10 + i as u8;
        t[b'A' as usize + i] = 10 + i as u8;
        i += 1;
    }
    t
}

/// Every byte's value as a hexadecimal digit, built at compile time.
const HEX: [u8; 256] = build_hex();

/// Whether `s[at]` begins `pct-encoded = "%" HEXDIG HEXDIG`.
fn escape_at(s: &[u8], at: usize) -> bool {
    match (s.get(at + 1), s.get(at + 2)) {
        (Some(&h), Some(&l)) => HEX[usize::from(h)] != NOT_HEX && HEX[usize::from(l)] != NOT_HEX,
        _ => false,
    }
}

/// Whether every byte of `s` is one `part` allows, each `%` beginning a
/// whole escape.
fn on_grammar(s: &[u8], part: Part) -> bool {
    let mut i = 0;
    while i < s.len() {
        match (CLASSES[usize::from(s[i])], part) {
            (Class::Name, _) | (Class::PathOnly | Class::Slash, Part::Path) => i += 1,
            (Class::Percent, _) if escape_at(s, i) => i += 3,
            _ => return false,
        }
    }
    true
}

/// Which component a byte run is checked as.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Host,
    Path,
}

/// `scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )` (RFC 3986 § 3.1).
fn is_scheme(s: &[u8]) -> bool {
    match s.split_first() {
        Some((first, rest)) => {
            first.is_ascii_alphabetic()
                && rest
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
        }
        None => false,
    }
}

/// `host`: a `reg-name` (which an IPv4 address also matches), or an
/// IP-literal in brackets. The literal's inside is held to the bytes
/// `IPvFuture` allows rather than to the IPv6 grammar: a literal is never
/// this machine's name, so it is a remote host whatever its digits, and the
/// finer grammar would only move it between "remote" and "malformed".
fn is_host(h: &[u8]) -> bool {
    match h.strip_prefix(b"[").and_then(|r| r.strip_suffix(b"]")) {
        Some(inside) => {
            !inside.is_empty()
                && inside
                    .iter()
                    .all(|&b| b == b':' || (CLASSES[usize::from(b)] == Class::Name))
        }
        None => on_grammar(h, Part::Host),
    }
}

/// `path-absolute`: a `/`, and no second one straight after it, since the
/// first segment after the root is `segment-nz`.
fn is_path_absolute(p: &[u8]) -> bool {
    p.first() == Some(&b'/') && p.get(1) != Some(&b'/') && on_grammar(p, Part::Path)
}

impl<'a> DirectoryReport<'a> {
    /// Read an `OSC 7` body, the bytes after `7;`.
    pub fn parse(pt: &'a [u8]) -> DirectoryReport<'a> {
        let Some(colon) = pt.iter().position(|&b| b == b':') else {
            return DirectoryReport::Malformed;
        };
        let (scheme, rest) = (&pt[..colon], &pt[colon + 1..]);
        if !scheme.eq_ignore_ascii_case(b"file") {
            return match is_scheme(scheme) {
                true => DirectoryReport::OtherScheme,
                false => DirectoryReport::Malformed,
            };
        }
        let (host, path) = match rest.strip_prefix(b"//") {
            Some(auth_path) => {
                let Some(slash) = auth_path.iter().position(|&b| b == b'/') else {
                    return DirectoryReport::Malformed;
                };
                let (h, p) = auth_path.split_at(slash);
                if !is_host(h) {
                    return DirectoryReport::Malformed;
                }
                let host = match h.is_empty() || h.eq_ignore_ascii_case(b"localhost") {
                    true => FileHost::Local,
                    false => FileHost::Named(h),
                };
                (host, p)
            }
            None => (FileHost::Local, rest),
        };
        match is_path_absolute(path) {
            true => DirectoryReport::File(FileUri { host, path }),
            false => DirectoryReport::Malformed,
        }
    }
}

impl<'a> FileUri<'a> {
    /// The path as written, still percent-encoded: every byte of it on the
    /// grammar, so ASCII.
    pub fn encoded_path(&self) -> &'a [u8] {
        self.path
    }

    /// The path, percent-decoded into `out` (`[[source: rfc3986]]` § 2.1,
    /// 634-647: "a character triplet, consisting of the percent character
    /// "%" followed by the two hexadecimal digits representing that octet's
    /// numeric value"). `None` when `out` is too short or the path decodes
    /// to a NUL, which no POSIX path can hold: "%00" would name a directory
    /// whose name ends early.
    pub fn decode_path<'o>(&self, out: &'o mut [u8]) -> Option<&'o [u8]> {
        let (mut i, mut n) = (0, 0);
        while i < self.path.len() {
            let b = match self.path[i] {
                b'%' => {
                    let v = HEX[usize::from(self.path[i + 1])] << 4
                        | HEX[usize::from(self.path[i + 2])];
                    i += 3;
                    v
                }
                b => {
                    i += 1;
                    b
                }
            };
            if b == 0 {
                return None;
            }
            *out.get_mut(n)? = b;
            n += 1;
        }
        Some(&out[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(pt: &[u8]) -> FileUri<'_> {
        match DirectoryReport::parse(pt) {
            DirectoryReport::File(f) => f,
            other => panic!("{pt:?} read as {other:?}"),
        }
    }

    /// `pt`'s path decoded, compared with `want`.
    fn path_is(pt: &[u8], want: &[u8]) {
        let mut out = [0u8; 512];
        assert_eq!(file(pt).decode_path(&mut out), Some(want), "{pt:?}");
    }

    #[test]
    fn apples_form_reads_its_host_and_path() {
        let f = file(b"file://mac.local/Users/me/My%20Code");
        assert_eq!(f.host, FileHost::Named(b"mac.local"));
        path_is(b"file://mac.local/Users/me/My%20Code", b"/Users/me/My Code");
    }

    #[test]
    fn no_host_and_localhost_are_this_machine() {
        assert_eq!(file(b"file:///tmp").host, FileHost::Local);
        assert_eq!(file(b"file://localhost/tmp").host, FileHost::Local);
        assert_eq!(file(b"file://LocalHost/tmp").host, FileHost::Local);
        assert_eq!(
            file(b"FILE:/tmp").host,
            FileHost::Local,
            "local-path, any case"
        );
        path_is(b"file:/tmp", b"/tmp");
    }

    #[test]
    fn utf8_is_decoded_byte_for_byte() {
        path_is(b"file:///caf%C3%A9", "/café".as_bytes());
        path_is(b"file:///caf%c3%a9", "/café".as_bytes());
    }

    #[test]
    fn off_the_grammar_is_malformed() {
        for pt in [
            &b""[..],
            b"/tmp",
            b"file:",
            b"file://host",
            b"file://host/a b",
            b"file:///caf\xc3\xa9",
            b"file:///a%2",
            b"file:///a%zz",
            b"file:///a?q",
            b"file:///a#f",
            b"file:////a",
            b"file:relative",
            b"file://ho st/a",
            b"file://host:22/a",
            b"file://[]/a",
            b"1file:///a",
        ] {
            assert_eq!(
                DirectoryReport::parse(pt),
                DirectoryReport::Malformed,
                "{pt:?}"
            );
        }
    }

    #[test]
    fn another_scheme_is_its_own_answer() {
        assert_eq!(
            DirectoryReport::parse(b"kitty-shell-cwd://host/a"),
            DirectoryReport::OtherScheme
        );
        assert_eq!(
            DirectoryReport::parse(b"http://host/a"),
            DirectoryReport::OtherScheme
        );
    }

    #[test]
    fn the_grammar_takes_its_own_odd_bytes() {
        path_is(
            b"file:///a:b@c/!$&'()*+,;=/-._~",
            b"/a:b@c/!$&'()*+,;=/-._~",
        );
        path_is(b"file:///a//b", b"/a//b");
        path_is(b"file:///", b"/");
        assert_eq!(file(b"file://[::1]/a").host, FileHost::Named(b"[::1]"));
        assert_eq!(
            file(b"file://10.0.0.1/a").host,
            FileHost::Named(b"10.0.0.1")
        );
    }

    #[test]
    fn a_nul_or_a_short_buffer_decodes_to_nothing() {
        let mut out = [0u8; 512];
        assert_eq!(file(b"file:///a%00b").decode_path(&mut out), None);
        let mut small = [0u8; 2];
        assert_eq!(file(b"file:///abc").decode_path(&mut small), None);
        let mut exact = [0u8; 4];
        assert_eq!(
            file(b"file:///abc").decode_path(&mut exact),
            Some(&b"/abc"[..])
        );
    }
}
