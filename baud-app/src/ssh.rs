//! **The ssh in front, read from its own argv, and the window that opens a
//! file on its host.** A file link a program printed inside an ssh
//! session names that session's machine, and the one way to reach it is
//! the connection itself, never the link's bytes: so the argv of the ssh
//! the kernel reports in front is read by ssh's own option grammar
//! ([`read`]), and the window that opens the file runs a second `ssh` with
//! that one's identity options to its destination ([`window_argv`]). Pure
//! over the argv and the link, so its tests run on every platform.
//!
//! **The grammar is ssh's own**: BSD `getopt` over
//! `1246ab:c:e:fgi:kl:m:no:p:qstvxAB:CD:E:F:GI:J:KL:MNO:P:Q:R:S:TVw:W:XYy`,
//! stopping at the first argument that is not an option; that one is the
//! destination, `[user@]host` or `ssh://[user@]host[:port]`; then, unless
//! `--` ended the options, `getopt` runs again over what follows, and
//! anything left after that is a command for the other machine. So `ssh
//! demo -p 2222` is a session and `ssh demo -- ls` is not.
//!
//! **Only an interactive session is one to open a file in**: an ssh that
//! runs a command of its own, or `-N`, `-W`, `-O`, `-G` or `-V`, has no
//! shell on the other end, and is refused. **Only identity options are
//! passed on** -- `-p`, `-l`, `-i`, `-J`, `-F`, `-4`, `-6`, `-C`, and `-o`
//! only for the keywords that say who and how to reach ([`O_KEPT`]) --
//! never a forward, a control master, a background fork, a log file or
//! a local command, which a second connection would duplicate or take over.
//!
//! **No interpolation.** The other machine's login shell reads the
//! command sshd hands it, so the window's remote command is a fixed
//! `/bin/sh` script ([`SCRIPT`]) followed by words each single-quoted on
//! its own ([`quoted`]): the link's host and the program with its path
//! reach the script only as `"$1"` and `"$@"`, never as shell text. A word
//! holding `\`, a control or invalid UTF-8 is refused, because a login
//! shell may read `\\` and `\'` inside single quotes; a `!` is written
//! outside the quotes as `\!`, because a C-family shell expands it as
//! history inside them, under `-c` too.
#![forbid(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};

/// **ssh's options**, as its `getopt` string spells them: a letter
/// followed by `:` takes an argument.
const OPTIONS: &[u8] = b"1246ab:c:e:fgi:kl:m:no:p:qstvxAB:CD:E:F:GI:J:KL:MNO:P:Q:R:S:TVw:W:XYy";

/// The flags that leave no shell on the other end: no session (`-N`), a
/// forwarded stream (`-W`), a control command (`-O`), the configuration
/// printed (`-G`), the version (`-V`).
const NOT_INTERACTIVE: &[u8] = b"NWOGV";

/// The options a window's ssh is given, each as the user's ssh had it:
/// who and where, and nothing that changes the session.
const IDENTITY: &[u8] = b"pliJFo46C";

/// **The `-o` keywords a window's ssh is given**, as `ssh_config(5)` names
/// them and ssh compares them, lowercased: who, where, how to reach and how
/// to authenticate, and `ControlPath` to ride a master the user already
/// has. Any other keyword is dropped: a forward, `ControlMaster` (never a
/// second master), a fork, a session type, a remote or local command, and
/// any keyword this list does not hold. `ssh_config` still applies its own.
const O_KEPT: &[&str] = &[
    "user",
    "port",
    "hostname",
    "identityfile",
    "identitiesonly",
    "certificatefile",
    "proxyjump",
    "proxycommand",
    "pubkeyauthentication",
    "preferredauthentications",
    "userknownhostsfile",
    "stricthostkeychecking",
    "addressfamily",
    "compression",
    "controlpath",
];

/// **An `-o` argument's keyword, as ssh reads it**: leading whitespace
/// skipped, ended by whitespace, `"` or `=`, lowercased.
fn o_keyword(arg: &OsStr) -> String {
    let b = arg.as_bytes();
    let ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\r' | b'\n');
    let from = b.iter().position(|c| !ws(c)).unwrap_or(b.len());
    let to = b[from..]
        .iter()
        .position(|c| ws(c) || matches!(c, b'"' | b'='))
        .map_or(b.len(), |n| from + n);
    String::from_utf8_lossy(&b[from..to]).to_ascii_lowercase()
}

/// One option letter of ssh's command line, as its `getopt` reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Flag(u8);

/// A position in a command line's arguments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ArgAt(usize);

/// Whether ssh's `getopt` takes an argument after `flag`; none when
/// the flag is not one of its own, which ssh refuses with its usage.
fn takes(flag: Flag) -> Option<bool> {
    let at = OPTIONS.iter().position(|b| *b == flag.0)?;
    Some(OPTIONS.get(at + 1) == Some(&b':'))
}

/// **A host's name, as two hosts are compared**: lowercase, one trailing
/// dot dropped, and either labels of letters, digits and `-` (a label 1 to
/// 63 bytes and not starting or ending with `-`, 253 bytes in all) or an
/// IP literal, an IPv6 one with or without its
/// brackets. Never empty.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HostName(String);

impl HostName {
    /// `s` as a host's name, or none when it is not one.
    pub fn of(s: &str) -> Option<HostName> {
        let s = s.strip_suffix('.').unwrap_or(s);
        let bare = s
            .strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .unwrap_or(s);
        if bare.parse::<std::net::Ipv6Addr>().is_ok() || bare.parse::<std::net::Ipv4Addr>().is_ok()
        {
            return Some(HostName(bare.to_ascii_lowercase()));
        }
        let ldh = |l: &str| {
            (1..=63).contains(&l.len())
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        };
        match !s.is_empty() && s.len() <= 253 && s.split('.').all(ldh) {
            true => Some(HostName(s.to_ascii_lowercase())),
            false => None,
        }
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// **Where an ssh connects, as its argv typed it**: the argument itself,
/// handed to the window's ssh as it was, so ssh resolves it the same way
/// (an alias of `ssh_config` included); and its host, when that is a
/// host's name -- an alias like `my_box` is not, and is matched only by
/// what `ssh -G` resolves it to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Destination {
    arg: OsString,
    host: Option<HostName>,
}

impl Destination {
    /// **`arg` as ssh reads a destination**, `[user@]host` or
    /// `ssh://[user@]host[:port][/...]`; none when ssh itself would refuse
    /// it: an empty host or user, a host starting with `-`, or one holding
    /// a blank, a control or one of ``'`"$\;&<>|(){}``.
    fn of(arg: &OsStr) -> Option<Destination> {
        let s = arg.to_str()?;
        let host = match s.strip_prefix("ssh://") {
            Some(rest) => {
                let (user, hostport) = match rest.split_once('@') {
                    Some((u, h)) => (Some(u.split(';').next().unwrap_or(u)), h),
                    None => (None, rest),
                };
                if user.is_some_and(str::is_empty) {
                    return None;
                }
                let hostport = hostport.split('/').next().unwrap_or(hostport);
                match hostport.strip_prefix('[') {
                    Some(v6) => v6.split(']').next().unwrap_or(v6),
                    None => hostport.split(':').next().unwrap_or(hostport),
                }
            }
            None => match s.rsplit_once('@') {
                Some(("", _)) => return None,
                Some((_, h)) => h,
                None => s,
            },
        };
        let refused =
            |c: char| "'`\"$\\;&<>|(){}".contains(c) || c.is_whitespace() || c.is_control();
        if host.is_empty() || host.starts_with('-') || host.chars().any(refused) {
            return None;
        }
        Some(Destination {
            arg: arg.to_owned(),
            host: HostName::of(host),
        })
    }

    /// The argument as typed.
    pub fn arg(&self) -> &OsStr {
        &self.arg
    }

    /// Its host, when the argument names one.
    #[cfg(test)]
    pub fn host(&self) -> Option<&HostName> {
        self.host.as_ref()
    }
}

/// **An interactive ssh, read**: the identity options its argv gave, each
/// flag and its argument as separate elements, in order, and where it
/// connects.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SshDest {
    options: Vec<OsString>,
    dest: Destination,
}

impl SshDest {
    /// The identity options, to pass on.
    #[cfg(test)]
    pub fn options(&self) -> &[OsString] {
        &self.options
    }

    /// Where it connects.
    pub fn dest(&self) -> &Destination {
        &self.dest
    }
}

/// Why an ssh's argv is not a session a file can be opened in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotSession {
    /// It runs a command of its own on the other machine.
    Command,
    /// One of [`NOT_INTERACTIVE`]'s flags.
    Flag(#[cfg_attr(not(test), allow(dead_code))] u8),
    /// No destination, or one ssh refuses.
    NoDestination,
    /// A flag ssh does not have, or one missing its argument: ssh
    /// itself would stop with its usage.
    Usage,
    /// The configuration runs `ssh` with no terminal (`RequestTTY no`, `-T`).
    NoTerminal,
    /// The configuration asks no shell (`SessionType none` or `subsystem`).
    NoShell,
    /// `ssh -G` printed nothing: no ssh to ask, or one that refuses its
    /// own configuration, as the session's would.
    Unresolved,
}

/// **The argv of the ssh in front, read by ssh's grammar**: `argv[0]`
/// is the program, which is not read.
pub fn read(argv: &[OsString]) -> Result<SshDest, NotSession> {
    let mut options = Vec::new();
    let mut flags = Vec::new();
    // The first `getopt`, from after the program's name.
    let (stop, ended) = getopt(argv, ArgAt(1), &mut options, &mut flags)?;
    let arg = argv.get(stop).ok_or(NotSession::NoDestination)?;
    let dest = Destination::of(arg).ok_or(NotSession::NoDestination)?;
    // Again over what follows the destination, unless `--` ended the
    // options: ssh's `goto again`.
    let command = match ended {
        Ended::Dashes => stop + 1,
        Ended::Operand => getopt(argv, ArgAt(stop + 1), &mut options, &mut flags)?.0,
    };
    if let Some(f) = flags.iter().find(|f| NOT_INTERACTIVE.contains(f)) {
        return Err(NotSession::Flag(*f));
    }
    if command < argv.len() {
        return Err(NotSession::Command);
    }
    Ok(SshDest { options, dest })
}

/// How a `getopt` pass ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ended {
    /// At an argument that is not an option, or the end.
    Operand,
    /// At `--`, which it took.
    Dashes,
}

/// **One BSD `getopt` pass over `argv` from `from`**: each flag seen into
/// `flags`, each identity flag and its argument into `options`; where it
/// stopped, and how.
fn getopt(
    argv: &[OsString],
    from: ArgAt,
    options: &mut Vec<OsString>,
    flags: &mut Vec<u8>,
) -> Result<(usize, Ended), NotSession> {
    let mut i = from.0;
    while let Some(a) = argv.get(i) {
        let b = a.as_bytes();
        if b == b"--" {
            return Ok((i + 1, Ended::Dashes));
        }
        if b.len() < 2 || b[0] != b'-' {
            return Ok((i, Ended::Operand));
        }
        i += 1;
        let mut j = 1;
        while j < b.len() {
            let f = b[j];
            j += 1;
            flags.push(f);
            let value = match takes(Flag(f)).ok_or(NotSession::Usage)? {
                false => None,
                true if j < b.len() => {
                    let v = &b[j..];
                    j = b.len();
                    Some(OsString::from_vec(v.to_vec()))
                }
                true => {
                    let v = argv.get(i).ok_or(NotSession::Usage)?.clone();
                    i += 1;
                    Some(v)
                }
            };
            let kept = match (f, &value) {
                (b'o', Some(v)) => O_KEPT.contains(&o_keyword(v).as_str()),
                _ => IDENTITY.contains(&f),
            };
            if kept {
                options.push(OsString::from_vec(vec![b'-', f]));
                options.extend(value);
            }
        }
    }
    Ok((i, Ended::Operand))
}

/// **What `ssh -G` resolved for a session**, from its own configuration
/// printout (a `keyword value` line each): the host it connects to, after
/// `ssh_config`'s `HostName` and canonicalisation, when that is a host's
/// name.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolved {
    hostname: Option<HostName>,
}

impl Resolved {
    /// The host it resolved to.
    #[cfg(test)]
    pub fn hostname(&self) -> Option<&HostName> {
        self.hostname.as_ref()
    }
}

/// **The argv that asks ssh what it would do**: `ssh -G`, the session's
/// identity options and its destination, run with no shell and no
/// terminal, on the paths worker, never on the main thread.
pub fn resolve_argv(ssh: &SshDest) -> Vec<OsString> {
    let mut out: Vec<OsString> = Vec::with_capacity(ssh.options.len() + 3);
    out.push("ssh".into());
    out.push("-G".into());
    out.extend(ssh.options.iter().cloned());
    out.push(ssh.dest.arg.clone());
    out
}

/// **`ssh -G`'s printout, read**: `hostname`, and whether the session is
/// one a file can be opened in -- a `remotecommand` of the configuration's
/// own runs that instead of a shell, `requesttty` `false` (ssh prints the
/// first spelling of its value, `-T`'s) gives no terminal, and a
/// `sessiontype` other than `default` (`none` for `-N`, `subsystem` for
/// `-s`) has no shell.
pub fn resolved(printout: &[u8]) -> Result<Resolved, NotSession> {
    let mut hostname = None;
    // The strongest refusal of the printout's, whatever order its lines
    // come in: a command of its own, then no shell, then no terminal.
    let mut refused: Option<NotSession> = None;
    let mut refuse = |why: NotSession| {
        let rank = |w: &NotSession| match w {
            NotSession::Command => 0,
            NotSession::NoShell => 1,
            _ => 2,
        };
        if refused.as_ref().is_none_or(|r| rank(&why) < rank(r)) {
            refused = Some(why);
        }
    };
    for line in printout.split(|b| *b == b'\n') {
        let Ok(line) = core::str::from_utf8(line) else {
            continue;
        };
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match (key, value) {
            ("hostname", v) => hostname = HostName::of(v),
            ("remotecommand", _) => refuse(NotSession::Command),
            ("requesttty", "false" | "no") => refuse(NotSession::NoTerminal),
            ("sessiontype", v) if v != "default" => refuse(NotSession::NoShell),
            _ => {}
        }
    }
    match refused {
        Some(why) => Err(why),
        None => Ok(Resolved { hostname }),
    }
}

/// **The ssh in front, as a file link of its session opens through it**:
/// its argv read by ssh's grammar and what `ssh -G` resolved for it. Read
/// once per process, on the paths worker (`probe.rs`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Session {
    ssh: SshDest,
    resolved: Resolved,
}

impl Session {
    /// Its argv, read.
    pub fn ssh(&self) -> &SshDest {
        &self.ssh
    }

    /// What `ssh -G` resolved.
    pub fn resolved(&self) -> &Resolved {
        &self.resolved
    }
}

/// **The session an ssh's `argv` runs**: the argv read ([`read`]), then
/// `ssh -G` asked through `resolve` with [`resolve_argv`]'s argv and its
/// printout read ([`resolved`]). `resolve` is never asked for an argv that
/// is no session; one that answers nothing is [`NotSession::Unresolved`].
pub fn session(
    argv: &[OsString],
    resolve: impl FnOnce(&[OsString]) -> Option<Vec<u8>>,
) -> Result<Session, NotSession> {
    let ssh = read(argv)?;
    let printout = resolve(&resolve_argv(&ssh)).ok_or(NotSession::Unresolved)?;
    let got = resolved(&printout)?;
    Ok(Session { ssh, resolved: got })
}

/// How a link of the session's names the session's machine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Named {
    /// By the destination as typed (`demo`).
    Typed,
    /// By the host `ssh -G` resolved the destination to.
    Resolved,
    /// By some other name, which only the machine can confirm as its own:
    /// [`SCRIPT`] compares it with what the machine calls itself, in the new
    /// window, before anything runs. A program on the far end that prints
    /// file links with its machine's own name names it this way when the
    /// user connected by an alias or an address.
    ByItsOwnName,
}

/// **Whether a link of the session's names the session's machine, and
/// how**: the destination as typed, the host ssh resolved it to, or a name
/// left for the machine itself to confirm. `localhost` and `here`, this
/// machine's own names, are none -- a link a remote program printed with
/// them names a machine the window's ssh does not reach -- unless the user
/// connected to that very name. [`SCRIPT`] checks every one again on the far
/// end.
pub fn names(
    ssh: &SshDest,
    resolved: &Resolved,
    here: &[HostName],
    link: &HostName,
) -> Option<Named> {
    if ssh.dest.host.as_ref() == Some(link) {
        return Some(Named::Typed);
    }
    if resolved.hostname.as_ref() == Some(link) {
        return Some(Named::Resolved);
    }
    let this = link.as_str() == "localhost" || here.contains(link);
    (!this).then_some(Named::ByItsOwnName)
}

/// **A `file://` link's path, decoded and checked**: absolute, its `%XX`
/// escapes decoded, UTF-8, and holding no
/// control, NUL or `\`, which [`quoted`] would refuse on the other end.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RemotePath(OsString);

impl RemotePath {
    /// `uri`'s host and path, a `file://` link, or none when it is not one
    /// whose path can go to another machine; an empty host is none.
    pub fn of_link(uri: &str) -> Option<(HostName, RemotePath)> {
        let rest = uri.get(..7).filter(|s| s.eq_ignore_ascii_case("file://"))?;
        let rest = &uri[rest.len()..];
        let slash = rest.find('/')?;
        let host = HostName::of(&rest[..slash])?;
        let path = decoded(&rest[slash..])?;
        let ok =
            path.starts_with('/') && !path.contains('\\') && !path.chars().any(char::is_control);
        ok.then(|| (host, RemotePath(OsString::from(path))))
    }

    /// The path, as the program on the other end is given it.
    pub fn as_os_str(&self) -> &OsStr {
        &self.0
    }
}

/// `s` with each `%XX` decoded, or none when an escape is malformed or the
/// bytes are not UTF-8.
fn decoded(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = core::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// **The fixed script the other machine's `/bin/sh` runs**: the node name
/// the machine gives itself (`uname -n`, which POSIX requires), lowercased
/// in the C locale as a [`HostName`] is, whole or its first label, against the link's host
/// in `$1`, and the program in `"$@"` only once they agree; a link naming
/// another machine says so and runs nothing. It holds no `\`, no `'` and
/// no `!`, so it single-quotes whole in any shell, and asks no resolver,
/// which can wait on DNS.
pub const SCRIPT: &str = "h=$(uname -n | LC_ALL=C tr A-Z a-z); [ \"$h\" = \"$1\" ] || \
     [ \"${h%%.*}\" = \"$1\" ] || \
     { echo \"baud: this link names $1, this connection is $h\"; exit 1; }; shift; exec \"$@\"";

/// Why a word cannot go to the other machine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unquotable {
    /// A `\`, which a login shell may read inside single quotes.
    Backslash,
    /// A control, NUL included.
    Control,
    /// Not UTF-8.
    Encoding,
}

/// **`w` single-quoted on its own**, a `'` inside it as `'\''` and a `!` as
/// `'\!'`, which POSIX shells and C-family shells all read as the one
/// character; or why it cannot be.
pub fn quoted(w: &OsStr) -> Result<OsString, Unquotable> {
    let s = w.to_str().ok_or(Unquotable::Encoding)?;
    if s.contains('\\') {
        return Err(Unquotable::Backslash);
    }
    if s.chars().any(char::is_control) {
        return Err(Unquotable::Control);
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("'\\''"),
            '!' => out.push_str("'\\!'"),
            c => out.push(c),
        }
    }
    out.push('\'');
    Ok(OsString::from(out))
}

/// **The program `w` as the other machine's login shell reads it**: as
/// [`quoted`], but a program under `~/` keeps the `~/` outside the quotes,
/// so that shell takes it against its own home folder, and the rest is one
/// quoted word. Only the program is read this way, as only a rule's
/// program is read under `~/`; an argument, and the link's path, which is
/// absolute, are quoted whole.
fn program_quoted(w: &OsStr) -> Result<OsString, Unquotable> {
    let Some(rest) = w.to_str().and_then(|s| s.strip_prefix("~/")) else {
        return quoted(w);
    };
    let mut out = OsString::from("~/");
    out.push(quoted(OsStr::new(rest))?);
    Ok(out)
}

/// **The argv of the window that opens a remote file**: `ssh`, the user's
/// identity options, `-t` for a terminal, the destination as typed, `--`,
/// then the remote command as words: `exec /bin/sh -c` and the quoted
/// [`SCRIPT`], `sh`, the link's host and `remote`'s elements, each quoted
/// on its own, the program under `~/` as [`program_quoted`] reads it. ssh
/// joins the words with spaces into the one command sshd hands the login
/// shell; nothing local runs a shell.
pub fn window_argv(
    ssh: &SshDest,
    host: &HostName,
    remote: &[OsString],
) -> Result<Vec<OsString>, Unquotable> {
    let mut out: Vec<OsString> = Vec::with_capacity(ssh.options.len() + remote.len() + 10);
    out.push("ssh".into());
    out.extend(ssh.options.iter().cloned());
    out.push("-t".into());
    out.push(ssh.dest.arg.clone());
    out.push("--".into());
    for w in ["exec", "/bin/sh", "-c"] {
        out.push(w.into());
    }
    out.push(quoted(OsStr::new(SCRIPT))?);
    out.push("sh".into());
    out.push(quoted(OsStr::new(host.as_str()))?);
    let mut words = remote.iter();
    if let Some(program) = words.next() {
        out.push(program_quoted(program)?);
    }
    for w in words {
        out.push(quoted(w)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    fn read_s(v: &[&str]) -> Result<SshDest, NotSession> {
        read(&os(v))
    }

    /// **ssh's argv is read by its own grammar, not by its last word**:
    /// `-p 2222 -i k demo`, an attached `-oUser=x`, `-J a demo` and options
    /// after the destination are sessions to `demo`; `demo -- ls` and `demo
    /// ls` run a command, and `-N` and `-W` are no session; a flag ssh does
    /// not have, or one missing its argument, is its usage. Named break: take
    /// the last argument as the destination -- `-p 2222 -i k demo` holds, and
    /// `demo -- ls` reads as a session to `ls`.
    #[test]
    fn an_ssh_argv_is_read_by_its_own_grammar() {
        let dest = |v: &[&str]| read_s(v).map(|s| s.dest().arg().to_owned());
        assert_eq!(
            dest(&["ssh", "-p", "2222", "-i", "k", "demo"]),
            Ok("demo".into())
        );
        assert_eq!(dest(&["ssh", "-oUser=x", "demo"]), Ok("demo".into()));
        assert_eq!(dest(&["ssh", "-J", "a", "demo"]), Ok("demo".into()));
        assert_eq!(dest(&["ssh", "demo", "-p", "2222"]), Ok("demo".into()));
        assert_eq!(dest(&["ssh", "-4vp2222", "j@demo"]), Ok("j@demo".into()));
        assert_eq!(dest(&["ssh", "--", "demo"]), Ok("demo".into()));
        assert_eq!(
            read_s(&["ssh", "demo", "--", "ls"]),
            Err(NotSession::Command)
        );
        assert_eq!(read_s(&["ssh", "demo", "ls"]), Err(NotSession::Command));
        assert_eq!(
            read_s(&["ssh", "-p", "22", "demo", "-v", "ls", "-l"]),
            Err(NotSession::Command)
        );
        assert_eq!(read_s(&["ssh", "-N", "demo"]), Err(NotSession::Flag(b'N')));
        assert_eq!(
            read_s(&["ssh", "demo", "-W", "h:1"]),
            Err(NotSession::Flag(b'W'))
        );
        for f in ["-O", "-G", "-V"] {
            assert!(
                matches!(read_s(&["ssh", f, "x", "demo"]), Err(NotSession::Flag(_))),
                "{f}"
            );
        }
        assert_eq!(read_s(&["ssh", "-Z", "demo"]), Err(NotSession::Usage));
        assert_eq!(read_s(&["ssh", "demo", "-p"]), Err(NotSession::Usage));
        assert_eq!(read_s(&["ssh", "-p", "22"]), Err(NotSession::NoDestination));
        assert_eq!(read_s(&["ssh", "a;b"]), Err(NotSession::NoDestination));
    }

    /// **Only identity options are passed on**: `-p` and `-i`, with their
    /// arguments, attached or not, `-4` and `-C` out of a cluster, and `-o`
    /// for a kept keyword however it is spelled (`Key=value`, `Key value`,
    /// leading blanks, any case), each as its own element in order; never
    /// `-L`, `-R`, `-D`, `-M`, `-S`, `-f`, `-E` or `-v`, and no `-o` that
    /// forwards, makes a master, forks, runs a local command or is not on the
    /// list. Named breaks: copy the argv's options whole -- the forward and
    /// the master come along; keep every `-o` -- `LocalForward` and
    /// `ControlMaster` come along.
    #[test]
    fn only_identity_options_are_passed_on() {
        let s = read_s(&[
            "ssh",
            "-L",
            "8080:x:80",
            "-M",
            "-S",
            "/tmp/c",
            "-p2222",
            "-4Cv",
            "-i",
            "k",
            "-R",
            "9:y:9",
            "-D",
            "1080",
            "-f",
            "-E",
            "log",
            "-oUser=x",
            "-oLocalForward=1:h:1",
            "-o",
            "ControlMaster=yes",
            "-o",
            "  proxycommand nc %h %p",
            "-oForkAfterAuthentication=yes",
            "-o",
            "LocalCommand=touch x",
            "-o",
            "PermitLocalCommand yes",
            "-oNoSuchKeyword=1",
            "-o",
            "ControlPath=/tmp/m",
            "demo",
        ])
        .expect("a session");
        assert_eq!(
            s.options(),
            os(&[
                "-p",
                "2222",
                "-4",
                "-C",
                "-i",
                "k",
                "-o",
                "User=x",
                "-o",
                "  proxycommand nc %h %p",
                "-o",
                "ControlPath=/tmp/m",
            ])
        );
        assert_eq!(
            read_s(&["ssh", "demo"]).expect("a session").options(),
            &[] as &[OsString]
        );
    }

    /// **A destination is read as ssh reads one, and its host compared as a
    /// host's name**: `user@host` and `ssh://user@host:port/x` give the
    /// host, lowered, a trailing dot gone, an IPv6 literal without brackets;
    /// an alias that is not a host's name keeps its argument and no host.
    #[test]
    fn a_destination_names_its_host() {
        let host = |s: &str| {
            Destination::of(OsStr::new(s)).and_then(|d| d.host().map(|h| h.as_str().to_owned()))
        };
        assert_eq!(host("demo").as_deref(), Some("demo"));
        assert_eq!(
            host("Jordan@Demo.Example.").as_deref(),
            Some("demo.example")
        );
        assert_eq!(host("ssh://j@demo:2222/x").as_deref(), Some("demo"));
        assert_eq!(host("ssh://[fe80::1]:22").as_deref(), Some("fe80::1"));
        assert_eq!(host("my_box"), None);
        assert!(
            Destination::of(OsStr::new("my_box")).is_some(),
            "an alias is a destination"
        );
        assert!(
            Destination::of(OsStr::new("@demo")).is_none(),
            "an empty user"
        );
        assert!(Destination::of(OsStr::new("ssh://@demo")).is_none());
        for bad in ["", "-demo", "a b", "a$b", "a`b"] {
            assert!(Destination::of(OsStr::new(bad)).is_none(), "{bad:?}");
        }
        assert_eq!(
            HostName::of("LOCALHOST").map(|h| h.0),
            Some("localhost".into())
        );
        for bad in ["", ".", "a..b", "-a", "a-", "a_b", &"x".repeat(64)] {
            assert_eq!(HostName::of(bad), None, "{bad:?}");
        }
    }

    /// **The window's argv is exactly `ssh <options> -t <dest> -- <words>`**,
    /// every word its own element and each of the remote ones quoted on its
    /// own, with no shell run here. Named break: join the remote words into
    /// one string -- the argv has one remote element, not eight.
    #[test]
    fn the_windows_argv_is_ssh_with_quoted_words() {
        let s = read_s(&["ssh", "-p", "2222", "demo"]).expect("a session");
        let host = HostName::of("demo").expect("a host");
        let got = window_argv(&s, &host, &os(&["hx", "/home/j/README.md"])).expect("quoted");
        let script = format!("'{SCRIPT}'");
        assert_eq!(
            got,
            os(&[
                "ssh",
                "-p",
                "2222",
                "-t",
                "demo",
                "--",
                "exec",
                "/bin/sh",
                "-c",
                &script,
                "sh",
                "'demo'",
                "'hx'",
                "'/home/j/README.md'",
            ])
        );
        assert!(
            !SCRIPT.contains('\\') && !SCRIPT.contains('\'') && !SCRIPT.contains('!'),
            "the script quotes whole"
        );
        assert!(
            SCRIPT.starts_with("h=$(uname -n | LC_ALL=C tr A-Z a-z);")
                && !SCRIPT.contains("hostname"),
            "the node name, lowered, and no resolver"
        );
    }

    /// **A word with `\`, a control or invalid UTF-8 is refused**, and a `'`
    /// or a `!` is closed, escaped and reopened. Named break: let `\`
    /// through.
    #[test]
    fn a_word_is_quoted_or_refused() {
        assert_eq!(quoted(OsStr::new("it's")), Ok(OsString::from("'it'\\''s'")));
        assert_eq!(quoted(OsStr::new("a!b")), Ok(OsString::from("'a'\\!'b'")));
        assert_eq!(quoted(OsStr::new("a\\b")), Err(Unquotable::Backslash));
        assert_eq!(quoted(OsStr::new("a\nb")), Err(Unquotable::Control));
        assert_eq!(quoted(OsStr::new("a\0b")), Err(Unquotable::Control));
        assert_eq!(
            quoted(OsStr::from_bytes(b"a\xffb")),
            Err(Unquotable::Encoding)
        );
    }

    /// A scratch folder for one shell test: a fake `uname` that prints
    /// `name`, and `show`, which writes each argument it was given, NUL
    /// after each, so the bytes the script's `exec "$@"` handed on are read
    /// back exactly. Written by `/bin/sh`, so no descriptor of either file
    /// stays open here for a child another test forks to inherit.
    fn scratch(tag: &str, name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("baud-ssh-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("bin")).expect("a scratch folder");
        let write = |file: &str, body: String| {
            let p = d.join("bin").join(file);
            let mut sh = std::process::Command::new("/bin/sh")
                .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
                .arg(&p)
                .stdin(std::process::Stdio::piped())
                .spawn()
                .expect("sh");
            std::io::Write::write_all(&mut sh.stdin.take().expect("its input"), body.as_bytes())
                .expect("the script");
            assert!(sh.wait().expect("sh").success());
        };
        write("uname", format!("#!/bin/sh\necho {name}\n"));
        write("show", "#!/bin/sh\nprintf '%s\\0' \"$@\"\n".to_string());
        d
    }

    /// The remote command as sshd hands it to the login shell: the words
    /// after `--`, joined by spaces, as ssh joins them.
    fn remote_command(argv: &[OsString]) -> OsString {
        let at = argv.iter().position(|a| a == "--").expect("--");
        let mut out = Vec::new();
        for (i, w) in argv[at + 1..].iter().enumerate() {
            if i > 0 {
                out.push(b' ');
            }
            out.extend_from_slice(w.as_bytes());
        }
        OsString::from_vec(out)
    }

    /// `command` run as `shell -c` runs it, as sshd does, with `dir`'s
    /// `bin` first on `PATH` and nothing of the user's: what it printed, and
    /// how it ended.
    fn login_shell(shell: &[&str], command: &OsStr, dir: &std::path::Path) -> (Vec<u8>, bool) {
        let out = std::process::Command::new(shell[0])
            .args(&shell[1..])
            .arg("-c")
            .arg(command)
            .env_clear()
            .env("HOME", dir)
            .env("ZDOTDIR", dir)
            .env("HISTFILE", "/dev/null")
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", dir.join("bin").display()),
            )
            .output()
            .expect("the shell");
        (out.stdout, out.status.success())
    }

    /// The login shells a remote may have that this system ships at those
    /// paths: macOS has all six. A Linux system is promised only `/bin/sh`
    /// at a fixed path, so that is the one run there; the others, and the
    /// shells neither system ships, are the VM's end-to-end run. Each listed shell
    /// must exist: a missing one fails the test rather than skipping it.
    #[cfg(target_os = "macos")]
    const SHELLS: &[&[&str]] = &[
        &["/bin/sh"],
        &["/bin/dash"],
        &["/bin/ksh"],
        &["/bin/zsh", "-f"],
        &["/bin/tcsh", "-f"],
        &["/bin/csh", "-f"],
    ];
    #[cfg(not(target_os = "macos"))]
    const SHELLS: &[&[&str]] = &[&["/bin/sh"]];

    /// **Every path reaches the script's `"$@"` as the same bytes, whatever
    /// the login shell**: paths holding a space, `'`, `$`, `;`, a backquote,
    /// `*`, `!`, `!!`, `"`, `&`, `|`, parentheses, braces, `<`, `>`, `?`,
    /// `[`, `~`, `#`, `%`, `=` and UTF-8, each alone, doubled and all at
    /// once, through each of [`SHELLS`] as `shell -c`. None is expanded,
    /// split, globbed or run. Named breaks: put the path inside the script's
    /// text -- the shells read its specials; leave `!` inside the quotes --
    /// the C-family shells expand it as history.
    #[test]
    fn every_path_reaches_the_program_as_its_bytes() {
        let specials = [
            " ",
            "'",
            "$",
            ";",
            "`",
            "*",
            "!",
            "!!",
            "\"",
            "&",
            "|",
            "(",
            ")",
            "{",
            "}",
            "<",
            ">",
            "?",
            "[",
            "~",
            "#",
            "%",
            "=",
            "é",
            "日本",
            "$(touch x)",
            "'$HOME'",
        ];
        let mut paths: Vec<String> = specials
            .iter()
            .flat_map(|c| [format!("/tmp/a{c}b"), format!("/tmp/{c}{c}/x{c}")])
            .collect();
        paths.push(format!("/tmp/{}", specials.concat()));
        let dir = scratch("paths", "demo");
        let s = read(&os(&["ssh", "demo"])).expect("a session");
        let host = HostName::of("demo").expect("a host");
        for shell in SHELLS {
            for path in &paths {
                let show = dir.join("bin").join("show").into_os_string();
                let argv = window_argv(&s, &host, &[show, OsString::from(path)]).expect("quoted");
                let (out, ok) = login_shell(shell, &remote_command(&argv), &dir);
                assert!(ok, "{shell:?} {path:?}: {}", String::from_utf8_lossy(&out));
                assert_eq!(out, [path.as_bytes(), b"\0"].concat(), "{shell:?} {path:?}");
            }
        }
        assert!(
            !dir.join("x").exists() && !std::path::Path::new("x").exists(),
            "nothing ran"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A program under `~/` is taken against the other machine's home
    /// folder**: its word keeps `~/` outside the quotes and the rest one
    /// quoted word, so each of [`SHELLS`], as the login shell, runs the
    /// program in its own `$HOME` with the path as its bytes; a `~/` inside
    /// an argument, and a program named `~` or under `~user/`, stay quoted
    /// whole. Named break: quote the program whole -- no shell finds
    /// `~/bin/show`, and nothing runs.
    #[test]
    fn a_program_under_home_runs_from_the_far_home() {
        let s = read(&os(&["ssh", "demo"])).expect("a session");
        let host = HostName::of("demo").expect("a host");
        let argv =
            window_argv(&s, &host, &os(&["~/bin/show", "~/x y", "/tmp/a b"])).expect("quoted");
        assert_eq!(
            &argv[argv.len() - 3..],
            os(&["~/'bin/show'", "'~/x y'", "'/tmp/a b'"]).as_slice()
        );
        for whole in ["~", "~j/bin/show", "/tmp/~/show"] {
            let one = window_argv(&s, &host, &os(&[whole, "/a"])).expect("quoted");
            assert_eq!(one[one.len() - 2], OsString::from(format!("'{whole}'")));
        }
        let dir = scratch("home", "demo");
        for shell in SHELLS {
            let (out, ok) = login_shell(shell, &remote_command(&argv), &dir);
            assert!(ok, "{shell:?}: {}", String::from_utf8_lossy(&out));
            assert_eq!(out, b"~/x y\0/tmp/a b\0", "{shell:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The other machine's name is checked before anything runs**: a
    /// connection whose node name is another machine's prints the line and
    /// ends 1, and the program is never run; one that is the link's host,
    /// whole or by its first label, in any case, runs it. Named breaks: drop
    /// the `||` chain -- the program runs on the wrong machine; ask
    /// `hostname` -- the node name is never read and nothing runs; compare
    /// the node name unlowered -- `Demo` and `DEMO.example` run nothing.
    #[test]
    fn a_link_naming_another_machine_runs_nothing() {
        let s = read(&os(&["ssh", "demo"])).expect("a session");
        let host = HostName::of("demo").expect("a host");
        let names = [
            ("demo", true),
            ("demo.example", true),
            ("Demo", true),
            ("DEMO.example", true),
            ("other", false),
            ("Other.Example", false),
        ];
        for (i, (name, runs)) in names.into_iter().enumerate() {
            let dir = scratch(&format!("check-{i}"), name);
            let mark = dir.join("ran");
            let argv = window_argv(&s, &host, &[OsString::from("touch"), mark.clone().into()])
                .expect("quoted");
            for shell in SHELLS {
                let _ = std::fs::remove_file(&mark);
                let (out, ok) = login_shell(shell, &remote_command(&argv), &dir);
                assert_eq!((ok, mark.exists()), (runs, runs), "{name} under {shell:?}");
                let said = format!(
                    "baud: this link names demo, this connection is {}\n",
                    name.to_ascii_lowercase()
                );
                match runs {
                    true => assert!(out.is_empty(), "{name}: {}", String::from_utf8_lossy(&out)),
                    false => assert_eq!(String::from_utf8_lossy(&out), said, "{shell:?}"),
                }
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// The lines `resolved` reads of three `ssh -F /dev/null -G` printouts
    /// of the system's ssh, as printed and in their order: `-p 2222
    /// jordan@demo`; that with `-T -o RemoteCommand=ls`; and with `-N`. The
    /// rest of each printout is left out: it names this machine's home.
    const G_PLAIN: &str = "host demo\nuser jordan\nhostname demo\nport 2222\n\
        controlmaster false\nrequesttty auto\nsessiontype default\n";
    const G_T: &str = "host demo\nuser jordan\nhostname demo\nport 22\ncontrolmaster false\n\
        requesttty false\nsessiontype default\nremotecommand ls\n";
    const G_N: &str = "host demo\nuser jordan\nhostname demo\nport 22\ncontrolmaster false\n\
        requesttty false\nsessiontype none\n";

    /// **`ssh -G`'s printout says where the session goes and whether it has
    /// a shell**: the plain session resolves to `demo`; a configuration's
    /// remote command, no terminal or no session is no session a file opens
    /// in. And the argv that asks is `ssh -G`, the identity options and the
    /// destination. Named break: skip the `remotecommand` line -- the `-T`
    /// printout reads as a session with no terminal rather than a command.
    #[test]
    fn ssh_g_says_where_the_session_goes() {
        let r = resolved(G_PLAIN.as_bytes()).expect("a session");
        assert_eq!(r.hostname().map(HostName::as_str), Some("demo"));
        assert_eq!(resolved(G_T.as_bytes()), Err(NotSession::Command));
        assert_eq!(
            resolved(G_N.as_bytes()),
            Err(NotSession::NoShell),
            "-N: no shell first"
        );
        assert_eq!(
            resolved(b"hostname x\nsessiontype subsystem\n"),
            Err(NotSession::NoShell)
        );
        let s = read(&os(&[
            "ssh",
            "-v",
            "-p",
            "2222",
            "-L",
            "1:h:1",
            "jordan@demo",
        ]))
        .expect("a session");
        assert_eq!(
            resolve_argv(&s),
            os(&["ssh", "-G", "-p", "2222", "jordan@demo"])
        );
    }

    /// **A session is its argv read and `ssh -G` asked once with the
    /// identity options only**; an argv that is no session asks nothing,
    /// and a printout that is none, or that refuses, is no session. Named
    /// breaks: ask `ssh -G` with the argv whole -- the forward reaches it;
    /// ask before reading the argv -- `ssh demo ls` asks.
    #[test]
    fn a_session_is_its_argv_and_one_ssh_g() {
        let mut asked: Vec<Vec<OsString>> = Vec::new();
        let argv = os(&["ssh", "-p", "2222", "-L", "1:h:1", "demo"]);
        let s = session(&argv, |a| {
            asked.push(a.to_vec());
            Some(G_PLAIN.as_bytes().to_vec())
        })
        .expect("a session");
        assert_eq!(asked, [os(&["ssh", "-G", "-p", "2222", "demo"])]);
        assert_eq!(s.ssh().dest().arg(), "demo");
        assert_eq!(
            s.resolved().hostname().map(HostName::as_str),
            Some("demo"),
            "what ssh -G resolved"
        );
        let mut none = 0;
        let refused = session(&os(&["ssh", "demo", "ls"]), |_| {
            none += 1;
            Some(G_PLAIN.as_bytes().to_vec())
        });
        assert_eq!(
            (refused, none),
            (Err(NotSession::Command), 0),
            "nothing asked"
        );
        assert_eq!(
            session(&argv, |_| None),
            Err(NotSession::Unresolved),
            "no printout"
        );
        assert_eq!(
            session(&argv, |_| Some(G_T.as_bytes().to_vec())),
            Err(NotSession::Command),
            "the configuration's own command"
        );
    }

    /// **A link names the session's machine by the destination as typed, the
    /// host ssh resolved it to, or a name left for the machine to confirm,
    /// and never by this machine's names**: `demo`, and the address an alias
    /// resolves to, match here; `demo-prod-01`, the machine's own name a
    /// program on the far end prints in its file links, goes to the far
    /// end's check; `localhost` and this
    /// machine's names are refused, unless they are what the user typed.
    /// Named breaks: compare with the link's host only -- `demo` no longer
    /// matches as typed; refuse every other name -- the far end's own name
    /// never opens.
    #[test]
    fn a_link_names_the_session_by_its_destination_or_what_ssh_resolved() {
        let host = |s: &str| HostName::of(s).expect("a host");
        let here = [host("here.example"), host("here")];
        let typed = read(&os(&["ssh", "demo"])).expect("a session");
        let alias = read(&os(&["ssh", "my_box"])).expect("a session");
        let to = |h: &str| {
            resolved(format!("hostname {h}\nsessiontype default\n").as_bytes()).expect("a session")
        };
        let named = |s: &SshDest, r: &str, l: &str| names(s, &to(r), &here, &host(l));
        assert_eq!(named(&typed, "demo", "demo"), Some(Named::Typed));
        assert_eq!(
            named(&typed, "demo", "DEMO."),
            Some(Named::Typed),
            "as a host's name"
        );
        assert_eq!(named(&alias, "10.0.0.7", "10.0.0.7"), Some(Named::Resolved));
        for own in ["demo-prod-01", "other.example"] {
            assert_eq!(
                named(&typed, "demo", own),
                Some(Named::ByItsOwnName),
                "{own}"
            );
            assert_eq!(
                named(&alias, "10.0.0.7", own),
                Some(Named::ByItsOwnName),
                "{own}"
            );
        }
        for this in ["localhost", "LOCALHOST", "here.example", "here"] {
            assert_eq!(named(&typed, "demo", this), None, "{this}");
            assert_eq!(named(&alias, "10.0.0.7", this), None, "{this}");
        }
        let to_here = read(&os(&["ssh", "here"])).expect("a session");
        assert_eq!(
            named(&to_here, "here", "here"),
            Some(Named::Typed),
            "typed wins"
        );
    }

    /// **A `file://` link's path is decoded and checked before it goes to
    /// another machine**: its host a host's name, its path absolute, `%XX`
    /// decoded to UTF-8; an empty host, a backslash, a control, a malformed
    /// escape or invalid UTF-8 is none. Named break: leave the path encoded
    /// -- `a%20b` reaches the program as itself.
    #[test]
    fn a_links_remote_path_is_decoded_and_checked() {
        let (h, p) = RemotePath::of_link("file://Demo/home/j/a%20b%C3%A9.md").expect("a path");
        assert_eq!(
            (h.as_str(), p.as_os_str()),
            ("demo", OsStr::new("/home/j/a bé.md"))
        );
        assert!(RemotePath::of_link("FILE://demo/x").is_some());
        for bad in [
            "file:///home/j/x",
            "file://demo/a%5Cb",
            "file://demo/a%0Ab",
            "file://demo/a%zz",
            "file://demo/a%ff",
            "file://demo",
            "https://demo/x",
        ] {
            assert_eq!(RemotePath::of_link(bad), None, "{bad}");
        }
    }
}
