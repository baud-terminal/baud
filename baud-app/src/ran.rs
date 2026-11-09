//! **What the shell says it runs**, read off the bytes as the reader takes
//! them: `OSC 133 ; C` with kitty's `cmdline=` or `cmdline_url=`
//! (`[[source: kitty-shell-integration]]`, "kitty also optionally supports
//! sending the cmdline going to be executed with `<OSC>133;C`"; fish sends
//! the second form itself, `[[source: fish-terminal-compat]]`), and the
//! `OSC 133 ; D` that ends the command.
//!
//! A command whose program is one not on this machine -- ssh and its kin,
//! the paths worker's own list (`probe::front_of`) -- makes every read
//! from its `C` to its `D` remote for a link's origin. The mark comes in
//! the stream before the command's first byte, so a job that starts,
//! prints a link and ends between two samples of the group in front is
//! still named, on both systems and with no call to the kernel.
//!
//! **A mark can only raise.** A remote program can print marks too, so
//! nothing in a mark lowers what the reads are taken as unless it carries
//! the token baud's integration made for its shell, which never leaves
//! that shell: `baud_token=` on its `C` and its `D`. A program on the other
//! end cannot end the command early with a `D` of its own. Before any
//! token has been seen -- a shell marking its own prompts, as fish 4 does,
//! or another integration -- a plain `D` ends it, and the samples and the
//! process watch are what is left against a program that prints one.
//!
//! **A `C` with no command line, or an empty one, names nothing**
//! ([`Named::Nothing`]): it neither opens a remote command nor ends one,
//! and the samples and the process watch decide. bash sends one when its
//! history kept no line for the command, as `history 1` is then another
//! command's; zsh when its history mechanism is not active.
//!
//! It is scanned on the thread taking the reads, as the `OSC 8`
//! introducer is (`probe::Sampler`), and runs beside the core's own reading
//! of the same marks, which keeps none of these options.

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
}

impl Named {
    /// What the `C` mark with `fields` names.
    fn of(fields: &[u8]) -> Named {
        command_line(fields).map_or(Named::Nothing, |line| names(&line))
    }
}

/// Whether a command the shell named not local is running, and the token
/// of the shell that named it, if it carried one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Open {
    #[default]
    No,
    Remote(Option<Token>),
}

/// Whether a read was written while a command the shell named not local
/// ran, at any point in it: the process watch's verdict's type.
pub use crate::probe::Watched as Marked;

/// **The scan of the reads for the shell's command marks**: one per
/// reader, carried across reads, so a mark split between two is read
/// whole.
#[derive(Clone, Debug)]
pub struct Commands {
    at: At,
    /// The body read so far, at most [`BODY`] bytes of it.
    body: Vec<u8>,
    open: Open,
    /// The token of the last `C` that carried one: once any has, a mark
    /// without it ends nothing.
    known: Option<Token>,
}

impl Default for Commands {
    fn default() -> Commands {
        Commands {
            at: At::Text,
            body: Vec::with_capacity(BODY),
            open: Open::No,
            known: None,
        }
    }
}

impl Commands {
    /// **Whether `bytes` were written, any of them, while a command the
    /// shell named not local ran**: open as the read began, or opened by a
    /// mark inside it -- a short ssh's `C`, its link and its `D` can all be
    /// one read.
    pub fn read(&mut self, bytes: &[u8]) -> Marked {
        let mut marked = self.open != Open::No;
        let mut i = 0;
        while i < bytes.len() {
            match self.at {
                At::Text => match bytes[i..].iter().position(|&b| b == 0x1b) {
                    Some(p) => {
                        i += p + 1;
                        self.at = At::Esc;
                    }
                    None => break,
                },
                At::Esc => {
                    if bytes[i] == b']' {
                        i += 1;
                        self.body.clear();
                        self.at = At::Body;
                    } else if bytes[i] != 0x1b {
                        self.at = At::Text;
                    } else {
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
                            self.ended();
                        }
                        0x1b => self.at = At::BodyEsc,
                        // CAN and SUB abandon it.
                        _ => self.at = At::Text,
                    }
                }
                At::BodyEsc => {
                    if bytes[i] == b'\\' {
                        i += 1;
                        self.at = At::Text;
                        self.ended();
                    } else {
                        self.at = At::Esc;
                    }
                }
            }
            marked |= self.open != Open::No;
        }
        match marked {
            true => Marked::Elsewhere,
            false => Marked::Nothing,
        }
    }

    /// A body ended: acted on if it is an `OSC 133` command mark.
    fn ended(&mut self) {
        let body = core::mem::take(&mut self.body);
        self.mark(&body);
        self.body = body;
    }

    /// The body of a mark, acted on if it is an `OSC 133` command mark.
    fn mark(&mut self, body: &[u8]) {
        let Some(rest) = body.strip_prefix(b"133;") else {
            return;
        };
        let (letter, fields) = match rest.iter().position(|&b| b == b';') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        let token = options(fields)
            .find(|(name, v)| *name == b"baud_token" && !v.is_empty())
            .map(|(_, v)| Token::of(v));
        match letter {
            b"C" => self.started(token, Named::of(fields)),
            b"D" => self.done(token),
            _ => {}
        }
    }

    /// A `C`: a command not local opens, and a local one carrying the
    /// token of what is open ends it, its shell having started another.
    /// One that names nothing changes nothing.
    fn started(&mut self, token: Option<Token>, named: Named) {
        if token.is_some() {
            self.known = token;
        }
        match (named, self.open) {
            (Named::Elsewhere, Open::Remote(Some(t))) if token != Some(t) => {}
            (Named::Elsewhere, _) => self.open = Open::Remote(token),
            (Named::Here, Open::Remote(_)) if self.ends(token) => self.open = Open::No,
            (Named::Here | Named::Nothing, _) => {}
        }
    }

    /// A `D`: what is open ends if the mark carries its shell's token, or
    /// with no token ever seen.
    fn done(&mut self, token: Option<Token>) {
        if self.open != Open::No && self.ends(token) {
            self.open = Open::No;
        }
    }

    /// Whether a mark carrying `token` may end what is open.
    fn ends(&self, token: Option<Token>) -> bool {
        match self.open {
            Open::No => false,
            Open::Remote(Some(t)) => token == Some(t),
            Open::Remote(None) => match self.known {
                None => true,
                Some(k) => token == Some(k),
            },
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

/// **The command line a `C` mark's fields carry**, decoded: kitty's
/// `cmdline=`, "encoded by %q", one shell word read back, or its
/// `cmdline_url=`, "UTF-8 URL %-escaped text". None without either.
pub(crate) fn command_line(fields: &[u8]) -> Option<Vec<u8>> {
    options(fields).find_map(|(name, v)| match name {
        b"cmdline_url" => Some(url_decoded(v)),
        b"cmdline" => Some(first_word(v).unwrap_or_default()),
        _ => None,
    })
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
/// worker's list (`probe::front_of`); nothing if it has none.
fn names(line: &[u8]) -> Named {
    let mut rest = line;
    let mut after_prefix = false;
    while let Some((word, next)) = word_at(rest) {
        rest = next;
        if is_assignment(&word) {
            continue;
        }
        if PREFIXES.contains(&word.as_slice()) {
            after_prefix = true;
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
            _ => Named::Here,
        };
    }
    Named::Nothing
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
        let mut one = Commands::default();
        let mut all = c("T", "ssh localhost cat x");
        all.extend_from_slice(LINK);
        all.extend_from_slice(&d("T"));
        assert_eq!(one.read(&all), Marked::Elsewhere, "one read");
        assert_eq!(one.read(b"$ "), Marked::Nothing, "the prompt after it");
        let mut each = Commands::default();
        assert_eq!(each.read(&c("T", "ssh localhost cat x")), Marked::Elsewhere);
        assert_eq!(each.read(LINK), Marked::Elsewhere, "the link");
        assert_eq!(
            each.read(&d("T")),
            Marked::Elsewhere,
            "the read that ends it"
        );
        assert_eq!(each.read(b"$ "), Marked::Nothing);
        let mut ls = Commands::default();
        let mut local = c("T", "ls --hyperlink");
        local.extend_from_slice(LINK);
        local.extend_from_slice(&d("T"));
        assert_eq!(ls.read(&local), Marked::Nothing, "the control: ls");
    }

    /// **A mark split across two reads at every cut is read whole**.
    /// Named break: start a fresh scan at each read -- a split mark is
    /// lost.
    #[test]
    fn a_mark_split_at_every_cut_is_read_whole() {
        let m = c("T", "ssh h");
        for cut in 1..m.len() {
            let mut s = Commands::default();
            let (a, b) = m.split_at(cut);
            let _ = s.read(a);
            let _ = s.read(b);
            assert_eq!(s.read(LINK), Marked::Elsewhere, "split after {cut}");
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
        let mut s = Commands::default();
        let _ = s.read(&c("T", "ssh h"));
        for forged in [
            b"\x1b]133;D;0\x07".to_vec(),
            d("X"),
            c("X", "ls"),
            b"\x1b]133;C;cmdline=ls\x07".to_vec(),
            b"\x1b]133;A;aid=1\x07".to_vec(),
        ] {
            let _ = s.read(&forged);
            assert_eq!(s.read(LINK), Marked::Elsewhere, "{forged:?}");
        }
        let _ = s.read(&d("T"));
        assert_eq!(s.read(LINK), Marked::Nothing, "the shell's own D");
        let mut fish = Commands::default();
        let _ = fish.read(b"\x1b]133;C;cmdline_url=ssh%20h\x07");
        assert_eq!(fish.read(LINK), Marked::Elsewhere, "fish's own C");
        let _ = fish.read(b"\x1b]133;D;0\x07");
        assert_eq!(fish.read(LINK), Marked::Nothing, "no token ever: a plain D");
    }

    /// **The shell's next command ends one whose `D` never came**: a `C`
    /// for `ls` with the same token. A `C` naming another remote program
    /// keeps it open. Named break: end only at a `D` -- the window stays
    /// remote.
    #[test]
    fn the_shells_next_command_ends_one_whose_end_never_came() {
        let mut s = Commands::default();
        let _ = s.read(&c("T", "ssh h"));
        let _ = s.read(&c("T", "mosh h"));
        assert_eq!(s.read(LINK), Marked::Elsewhere, "another remote one");
        let _ = s.read(&c("T", "ls"));
        assert_eq!(s.read(LINK), Marked::Nothing, "the next command");
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
            let mut open = Commands::default();
            let _ = open.read(&c("T", "ssh h"));
            let _ = open.read(bare);
            assert_eq!(open.read(LINK), Marked::Elsewhere, "after an ssh: {bare:?}");
            let mut none = Commands::default();
            let _ = none.read(&c("T", "ls"));
            let _ = none.read(bare);
            assert_eq!(none.read(LINK), Marked::Nothing, "none open: {bare:?}");
        }
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
                assert_eq!(names(line.as_bytes()) == Named::Elsewhere, want, "{line:?}");
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
                    names(&url_decoded(url.as_bytes())) == Named::Elsewhere,
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
        assert_eq!(names(b""), Named::Nothing, "nothing");
        assert_eq!(names(b"FOO=ssh"), Named::Nothing, "an assignment alone");
        assert!(
            names("caf\u{e9}=1 ssh h".as_bytes()) == Named::Here,
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
            let mut scan = Commands::default();
            assert_eq!(
                scan.read(c.as_bytes()),
                Marked::Elsewhere,
                "{} bytes",
                c.len()
            );
            assert_eq!(
                scan.read(b"\x1b]133;D;3;baud_token=t\x07"),
                Marked::Elsewhere
            );
            assert_eq!(scan.read(b"$ "), Marked::Nothing, "closed by the shell's D");
        }
    }
}
