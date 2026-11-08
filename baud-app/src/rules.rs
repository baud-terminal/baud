//! **`[link.*]`: your own link types, and what a click on one does.** A rule
//! names a scheme baud does not open by default, `tel` or `ssh`, and sends
//! its links to the system's handler (`action = "open"`) or to a program the
//! config names (`action = "command"`); or it names one of the four default
//! schemes and changes only what a click on it runs. `[link.path]` turns on
//! the printed-path forms that are off by default and may hand a clicked path
//! to your editor. Pure over the config's text and the bytes of a link, so its
//! tests run on every platform and the `frame` bench compiles it as it stands.
//!
//! **The rule comes from you; the link comes from the program.** The config
//! file is yours: a program cannot write it, and nothing here is read from a
//! theme file, an escape sequence or the environment (`Config::apply` is the
//! one caller of [`read`]). So a rule may widen which schemes reach a
//! handler, and nothing it says loosens what is checked about a link's bytes:
//!
//! * **A rule baud does not understand whole is refused whole**, and said:
//!   an unknown key, an unknown value, a list it cannot read. Everywhere else
//!   in the config an unknown key is skipped and the rest applies; here that
//!   would let an older baud drop a key a newer one reads as a safeguard and
//!   run the command anyway.
//! * **No scheme that carries its own code**: `javascript`, `data`,
//!   `vbscript`, `blob` and `about` are refused, and so is any name outside
//!   RFC 3986's grammar, "a letter and followed by any combination of
//!   letters, digits, plus ("+"), period ("."), or hyphen ("-")"
//!   (`[[source: rfc3986]]` § 3.1), so no wildcard can be written.
//! * **No part of a link that starts with `-`** reaches a handler or a
//!   command ([`dashless`]): a host, a user or a port spelled `-o...` is the
//!   shape of CVE-2017-1000117, where an `ssh://-oProxyCommand=...` link ran
//!   any program on the machine.
//! * **No shell.** `command` is an argv run with no shell between, its
//!   standard streams the null device and reaped without waiting, as
//!   `opener.rs` runs `xdg-open`; `command[0]` is literal, never a
//!   placeholder; and a placeholder in the script a shell or an interpreter
//!   runs is refused at read ([`scripted`]). The list of programs that run
//!   their arguments as code cannot be complete, so the guide says why.
//! * **The defaults' own rules stay**: a `file` link names this machine or
//!   none, and a `mailto` carries only `subject` and `body`, whichever handler
//!   or command takes it (`link::admit_with`). A `file` link and a printed
//!   path are shown in Finder, never opened by the system: only a command of
//!   yours opens one.
//! * **The address still shows under the modifier**, in the same opaque bar,
//!   for every rule (`link::preview`): no key turns it off.
//!
//! **Order.** An `OSC 8` link is its cells' and no plain-text rule's; then a
//! scheme is yours, the first rule naming it in the file, before baud's own;
//! a cell belongs to one link, the first that starts at or before it and
//! reaches it (`link::find`).
#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::link::{Refused, Unit};
use crate::paths::{Forms, Turned};

/// The most rules read; the rest are refused and said. Each is a compare at
/// a `:` the frame's scan meets, so the number is kept small.
pub const MOST: usize = 32;

/// The longest scheme a rule may name.
pub const SCHEME_MAX: usize = 32;

/// **The keys of a `[link.NAME]` rule**, the one list the reader, the guide
/// and the example are held to.
pub const RULE_KEYS: [&str; 3] = ["scheme", "action", "command"];

/// **The keys of `[link.path]`**, the printed-path rule.
pub const PATH_KEYS: [&str; 5] = ["forms", "line_col", "spaces", "action", "command"];

/// The name the printed-path rule is read under; no scheme rule takes it.
pub const PATH_RULE: &str = "path";

/// Schemes whose links carry code of their own, or a page the terminal
/// itself would show: never a rule's.
pub const REFUSED_SCHEMES: [&str; 5] = ["javascript", "data", "vbscript", "blob", "about"];

/// The printed-path forms `[link.path] forms` names, in [`Forms`]' order.
pub const FORM_NAMES: [&str; 5] = ["/", "~/", "./", "../", "relative"];

/// The placeholder a scheme rule's command takes the link in.
pub const URL: &str = "{url}";
/// The placeholders `[link.path]`'s command takes: the path, and the
/// `:line:col` printed after it, `1` when none was.
pub const PATH: &str = "{path}";
pub const LINE: &str = "{line}";
pub const COL: &str = "{col}";

/// **How a program runs an argument as code**, for [`scripted`]: a
/// placeholder there is the link's bytes in a script, which no check of
/// those bytes makes safe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Code {
    /// The element after one of these short flags, or a long flag of
    /// [`SCRIPT_LONG`], is a script: shells and interpreters.
    Flag(&'static [char]),
    /// Its first operand is its program or a script file, unless one of
    /// `flags` gave it one; `values` are the short flags that take an
    /// argument, skipped to find the operand: `awk`, `sed` (whose `e`
    /// command runs a shell), `tclsh`, `Rscript`.
    Operand {
        flags: &'static [char],
        values: &'static str,
    },
    /// What follows its destination goes to a shell on another machine;
    /// `values` are the short flags that take an argument, skipped to find
    /// the destination: `ssh` and its kin.
    Remote { values: &'static str },
    /// Any element after it may be code, or a module it fetches and runs,
    /// after a subcommand (`deno eval`, `deno run`, `bun run`): `deno`, `bun`.
    Every,
}

/// `ssh`'s flags that take an argument (`ssh(1)`'s synopsis).
const SSH_ARGS: &str = "BbcDEeFIiJLlmOoPpQRSWw";

/// **The programs [`scripted`] knows, by file name**: a name ending in a
/// version, `python3.12` or `perl5.36`, is read as the name before it
/// ([`program_of`]). A program may have more than one way in: `et` takes a
/// command after `-c` and after its destination. The list cannot be
/// complete, and the guide says so.
const PROGRAMS: [(&str, Code); 37] = [
    ("sh", Code::Flag(&['c'])),
    ("bash", Code::Flag(&['c'])),
    ("zsh", Code::Flag(&['c'])),
    ("dash", Code::Flag(&['c'])),
    ("ksh", Code::Flag(&['c'])),
    ("mksh", Code::Flag(&['c'])),
    ("ash", Code::Flag(&['c'])),
    ("yash", Code::Flag(&['c'])),
    ("fish", Code::Flag(&['c'])),
    ("csh", Code::Flag(&['c'])),
    ("tcsh", Code::Flag(&['c'])),
    ("nu", Code::Flag(&['c'])),
    ("pwsh", Code::Flag(&['c', 'C', 'e', 'E'])),
    ("powershell", Code::Flag(&['c', 'C', 'e', 'E'])),
    // `-m` runs a module the argument names.
    ("python", Code::Flag(&['c', 'm'])),
    ("perl", Code::Flag(&['e', 'E'])),
    ("ruby", Code::Flag(&['e'])),
    ("node", Code::Flag(&['e', 'p'])),
    ("bun", Code::Flag(&['e', 'p'])),
    ("bun", Code::Every),
    ("deno", Code::Every),
    ("osascript", Code::Flag(&['e'])),
    ("lua", Code::Flag(&['e'])),
    ("php", Code::Flag(&['r'])),
    (
        "Rscript",
        Code::Operand {
            flags: &['e'],
            values: "",
        },
    ),
    (
        "tclsh",
        Code::Operand {
            flags: &[],
            values: "",
        },
    ),
    (
        "awk",
        Code::Operand {
            flags: &['f'],
            values: "Fv",
        },
    ),
    (
        "gawk",
        Code::Operand {
            flags: &['f', 'e'],
            values: "FvlLiE",
        },
    ),
    (
        "mawk",
        Code::Operand {
            flags: &['f'],
            values: "FvW",
        },
    ),
    (
        "nawk",
        Code::Operand {
            flags: &['f'],
            values: "Fv",
        },
    ),
    (
        "sed",
        Code::Operand {
            flags: &['e', 'f'],
            values: "l",
        },
    ),
    (
        "gsed",
        Code::Operand {
            flags: &['e', 'f'],
            values: "l",
        },
    ),
    ("ssh", Code::Remote { values: SSH_ARGS }),
    (
        "autossh",
        Code::Remote {
            values: "BbcDEeFIiJLlMmOoPpQRSWw",
        },
    ),
    ("mosh", Code::Remote { values: "p" }),
    ("et", Code::Remote { values: "uptxrjlc" }),
    ("et", Code::Flag(&['c'])),
];

/// Long flags that take a script, for the [`Code::Flag`] programs.
const SCRIPT_LONG: [&str; 5] = [
    "--command",
    "--eval",
    "--print",
    "-command",
    "-encodedcommand",
];

/// **The name [`PROGRAMS`] knows an element by**: its file name, and for a
/// name ending in a version -- digits and dots, `python3.12`, `perl5.36`,
/// `tclsh8.6` -- the name before it, so a version hides nothing.
fn program_of(e: &str) -> &str {
    let name = e.rsplit('/').next().unwrap_or(e);
    if PROGRAMS.iter().any(|(n, _)| *n == name) {
        return name;
    }
    let bare = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    match bare.len() < name.len() && !bare.is_empty() {
        true => bare,
        false => name,
    }
}

/// `env`'s flags that split their argument into words, which would make a
/// placeholder there a list of arguments, an option among them.
const ENV_SPLIT: [&str; 2] = ["-S", "--split-string"];

/// What a rule does with a link a click opens.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Act {
    /// The system's handler for the scheme: LaunchServices on macOS,
    /// `xdg-open` on Linux.
    Open,
    /// Your program.
    Command(Argv),
    /// Nothing: no line at rest, and a click opens nothing; the bar says
    /// it is off by your config. A default's scheme can be turned off too.
    Off,
    /// The link's text to the clipboard, through the copy a selection takes
    /// and its signal; nothing is opened and no process started.
    Copy,
}

/// What a click on a printed path does.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum PathAct {
    /// Shown in Finder (the file manager on Linux), never opened: the
    /// default.
    #[default]
    Reveal,
    Command(Argv),
    /// No printed path read at all, no lookup and no worker: what `[link]
    /// paths = "off"` does (`config::Link::paths_read`).
    Off,
    /// The path, as found, to the clipboard, after the click's check; nothing
    /// is opened.
    Copy,
}

/// **A command, as the config names it**: `command[0]` the program, by its
/// absolute path once read (`~/` taken against the home folder) or a name
/// looked up on `PATH` when it runs, and the rest its arguments, with their
/// placeholders.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Argv(Vec<String>);

/// Whether a rule's program is there to run: what the strike asks, off the
/// frame ([`Argv::program`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Program {
    Found,
    Missing,
}

impl Argv {
    /// The program, as written or made absolute.
    pub fn program(&self) -> &str {
        &self.0[0]
    }

    /// **Whether the program is there**: an executable file at its path, or
    /// on `path_var`'s folders for a bare name, as `execvp` would find it --
    /// the search `pty.rs` makes for a shell. A metadata call: never on the
    /// frame path. `path_var` is the `PATH` the command would run under,
    /// which for a baud started from the Dock is launchd's short one.
    pub fn found(&self, path_var: Option<&OsStr>) -> Program {
        program_found(self.program(), path_var)
    }
}

/// [`Argv::found`] for a program spelled `prog`.
pub fn program_found(prog: &str, path_var: Option<&OsStr>) -> Program {
    use std::os::unix::fs::PermissionsExt;
    let runs = |p: &Path| {
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let found = match prog.contains('/') {
        true => runs(Path::new(prog)),
        false => path_var.is_some_and(|v| std::env::split_paths(v).any(|d| runs(&d.join(prog)))),
    };
    match found {
        true => Program::Found,
        false => Program::Missing,
    }
}

/// **One `[link.NAME]` rule, read.**
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rule {
    name: String,
    /// Lowercase, as RFC 3986 makes canonical.
    scheme: String,
    act: Act,
    /// For a scheme baud does not know by default, the scan's prefilter;
    /// none for one of its own four, which its own list finds.
    suffix: Option<Suffix>,
}

/// **The scan's tail**: the last eight ASCII bytes it read, lowered, the
/// newest in the low byte, a byte outside ASCII a zero (`lined::Reader::wants`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Tail(pub(crate) u64);

/// **A scheme's last seven bytes before its `:`, packed as [`Tail`] packs
/// them, and the mask that compares that many**: whether a tail ending in
/// `:` may end the scheme's name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Suffix(u64, u64);

impl Suffix {
    fn of(scheme: &str) -> Suffix {
        let tail = &scheme.as_bytes()[scheme.len().saturating_sub(7)..];
        let packed = tail.iter().fold(0u64, |a, b| (a << 8) | u64::from(*b));
        Suffix(packed, (1u64 << (8 * tail.len())) - 1)
    }

    fn ends(self, tail: Tail) -> bool {
        (tail.0 >> 8) & self.1 == self.0
    }
}

/// **A rule's place in [`Rules`]**, in file order: what a [`crate::link::Target`]
/// it admitted carries to the click, and no other number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RuleIx(usize);

/// Whether any rule adds a scheme baud does not know by default.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Added {
    No,
    Yes,
}

impl Rule {
    #[cfg(test)]
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn act(&self) -> &Act {
        &self.act
    }
}

/// **Every `[link.*]` rule, in file order**, and the printed paths' forms and
/// click. Empty by default: what baud does out of the box.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rules {
    list: Vec<Rule>,
    /// Whether [`Rules::list`] holds a scheme baud does not know by default:
    /// whether the text is read for any beyond its own four.
    added: Added,
    forms: Forms,
    path: PathAct,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules::NONE
    }
}

/// No rules, for a read that has none to go by.
pub static NO_RULES: Rules = Rules::NONE;

impl Rules {
    /// No rule: baud's own four schemes and three path forms.
    pub const NONE: Rules = Rules {
        list: Vec::new(),
        added: Added::No,
        forms: Forms::DEFAULT,
        path: PathAct::Reveal,
    };

    /// The rules, in file order.
    #[cfg(test)]
    pub fn list(&self) -> &[Rule] {
        &self.list
    }

    /// The printed-path forms read.
    pub fn forms(&self) -> Forms {
        self.forms
    }

    /// What a click on a printed path does.
    pub fn path_act(&self) -> &PathAct {
        &self.path
    }

    /// **The rule for scheme `name`**, the first in the file that names it,
    /// compared without case (§ 3.1), and its place.
    pub fn rule_for(&self, name: &str) -> Option<(RuleIx, &Rule)> {
        self.list
            .iter()
            .enumerate()
            .find(|(_, r)| r.scheme.eq_ignore_ascii_case(name))
            .map(|(ix, r)| (RuleIx(ix), r))
    }

    /// The rule at `ix`, as [`Rules::rule_for`] gave it.
    pub fn at(&self, ix: RuleIx) -> Option<&Rule> {
        self.list.get(ix.0)
    }

    /// **Whether `tail`, the scan's last eight ASCII bytes, lowered, ending
    /// in `:`, may end the name of a scheme a rule added**: exact for a name
    /// of seven bytes or fewer, its last seven for a longer one, which the
    /// read then checks whole. Nothing to compare without an added scheme,
    /// so a config with none pays one compare at a `:` and no more.
    #[inline]
    pub(crate) fn ends(&self, tail: Tail) -> bool {
        self.added == Added::Yes
            && self
                .list
                .iter()
                .any(|r| r.suffix.is_some_and(|s| s.ends(tail)))
    }

    /// **The rule whose added scheme's name starts `rest`**, a `:` after
    /// it: a scheme baud knows by default is `link::KNOWN`'s, and is not
    /// looked for here.
    pub(crate) fn added_at(&self, rest: &[Unit]) -> Option<&Rule> {
        if self.added == Added::No {
            return None;
        }
        self.list.iter().filter(|r| r.suffix.is_some()).find(|r| {
            let n = r.scheme.len();
            rest.len() > n
                && r.scheme
                    .chars()
                    .zip(rest)
                    .all(|(c, u)| matches!(u, Unit::Char(x) if x.eq_ignore_ascii_case(&c)))
                && rest[n] == Unit::Char(':')
        })
    }

    /// **The programs your `[link.NAME]` rules run**: each `command` rule's
    /// `command[0]`, as [`Argv::program`] spells it, in file order, for the
    /// worker to look up. `[link.path]`'s is not among them.
    pub fn command_programs(&self) -> impl Iterator<Item = &str> {
        self.list.iter().filter_map(|r| match r.act() {
            Act::Command(argv) => Some(argv.program()),
            Act::Open | Act::Off | Act::Copy => None,
        })
    }

    /// **The program a click on scheme `name`'s links would run, and whether
    /// it is there** -- none where no rule of yours runs a command for it.
    /// The strike's question: a metadata call, so never on the frame path,
    /// and asked by the worker that probes paths, which no driver has wired
    /// to it yet.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn command_found(&self, name: &str, path_var: Option<&OsStr>) -> Option<Program> {
        match self.rule_for(name)?.1.act() {
            Act::Command(argv) => Some(argv.found(path_var)),
            Act::Open | Act::Off | Act::Copy => None,
        }
    }

    /// [`Rules::command_found`] for a click on a printed path.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn path_command_found(&self, path_var: Option<&OsStr>) -> Option<Program> {
        match &self.path {
            PathAct::Command(argv) => Some(argv.found(path_var)),
            PathAct::Reveal | PathAct::Off | PathAct::Copy => None,
        }
    }

    /// **The command a click on `uri`, admitted under rule `ix`, runs**:
    /// `{url}` replaced by the whole link, no shell, the standard streams the
    /// null device. The link's bytes are checked again where they land
    /// ([`landed`]); none if the rule runs no command.
    pub fn command(&self, ix: RuleIx, uri: &str) -> Option<Result<Command, Refused>> {
        let Act::Command(argv) = self.at(ix)?.act() else {
            return None;
        };
        Some(build(argv, &[(URL, uri.as_bytes(), Value::Uri)]))
    }

    /// **The command a click on printed path `path` runs**, the `:line:col`
    /// printed after it in `line` and `col`, empty when none was, so `1`;
    /// none when a click shows the file instead.
    pub fn path_command(
        &self,
        path: &[u8],
        line: &str,
        col: &str,
    ) -> Option<Result<Command, Refused>> {
        let PathAct::Command(argv) = &self.path else {
            return None;
        };
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let line = if digits(line) { line } else { "1" };
        let col = if digits(col) { col } else { "1" };
        if path.first() != Some(&b'/') {
            return Some(Err(Refused::Host));
        }
        Some(build(
            argv,
            &[
                (PATH, path, Value::Path),
                (LINE, line.as_bytes(), Value::Uri),
                (COL, col.as_bytes(), Value::Uri),
            ],
        ))
    }

    /// **What baud says once a load reads these rules**, a line each, for the
    /// log: adding a scheme stays a step you can see.
    pub fn told(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .list
            .iter()
            .map(|r| match &r.act {
                Act::Open => format!(
                    "config: [link.{}] sends {}: links from program output to the system's handler",
                    r.name, r.scheme
                ),
                Act::Command(a) => format!(
                    "config: [link.{}] sends {}: links from program output to {}",
                    r.name,
                    r.scheme,
                    a.program()
                ),
                Act::Off => format!("config: [link.{}] turns {}: links off", r.name, r.scheme),
                Act::Copy => format!(
                    "config: [link.{}] copies a clicked {}: link to the clipboard",
                    r.name, r.scheme
                ),
            })
            .collect();
        match &self.path {
            PathAct::Command(a) => out.push(format!(
                "config: [link.path] sends a clicked printed path to {}",
                a.program()
            )),
            PathAct::Off => out.push("config: [link.path] turns printed paths off".into()),
            PathAct::Copy => out
                .push("config: [link.path] copies a clicked printed path to the clipboard".into()),
            PathAct::Reveal => {}
        }
        out
    }
}

/// What a substituted value is, and so how its bytes are checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Value {
    /// A link, or digits: printable ASCII and `LINK_MAX` at most.
    Uri,
    /// A file name: UTF-8 with no C0, C1, DEL or bidirectional control.
    Path,
}

/// **Whether `v`'s bytes may land in an argument**: what `admit` holds a link
/// to, held again after substitution, and for a path what `paths.rs` holds a
/// printed one to; and never a leading `-`, whatever it is.
fn landed(v: &[u8], what: Value) -> Result<(), Refused> {
    if v.first() == Some(&b'-') {
        return Err(Refused::Dash);
    }
    match what {
        Value::Uri => {
            if v.is_empty() || v.len() > crate::link::MAX {
                return Err(Refused::Length);
            }
            match v.iter().all(|b| (0x21..=0x7e).contains(b)) {
                true => Ok(()),
                false => Err(Refused::Byte),
            }
        }
        Value::Path => {
            let s = core::str::from_utf8(v).map_err(|_| Refused::Byte)?;
            match s.chars().any(|c| c.is_control() || bidi_control(c)) {
                true => Err(Refused::Byte),
                false => Ok(()),
            }
        }
    }
}

/// A bidirectional control, which can make a name read as another
/// (`[[source: uax9-tr9-52]]`, the explicit formatting characters and the
/// marks): the set `paths.rs` refuses a printed path for.
fn bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// `argv` with each placeholder replaced by its value, checked, as a
/// [`Command`] with no shell and null standard streams.
fn build(argv: &Argv, values: &[(&str, &[u8], Value)]) -> Result<Command, Refused> {
    use std::os::unix::ffi::OsStringExt;
    for (_, v, what) in values {
        landed(v, *what)?;
    }
    let mut out: Vec<std::ffi::OsString> = Vec::with_capacity(argv.0.len());
    for (i, e) in argv.0.iter().enumerate() {
        // `command[0]` is literal: a placeholder there was refused at read.
        if i == 0 {
            out.push(e.into());
            continue;
        }
        let (bytes, put) = substituted(e.as_bytes(), values);
        // An element is checked whole once a value is in it: nothing the
        // config wrote can hold a control either, but this is the bytes that
        // are handed over.
        if put && landed_element(&bytes).is_err() {
            return Err(Refused::Byte);
        }
        out.push(std::ffi::OsString::from_vec(bytes));
    }
    let mut c = Command::new(&out[0]);
    c.args(&out[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(c)
}

/// An argument holding a substituted value: UTF-8, no control and no
/// bidirectional control.
fn landed_element(b: &[u8]) -> Result<(), Refused> {
    let s = core::str::from_utf8(b).map_err(|_| Refused::Byte)?;
    match s.chars().any(|c| c.is_control() || bidi_control(c)) {
        true => Err(Refused::Byte),
        false => Ok(()),
    }
}

/// **`element` with each placeholder replaced by its value, in one pass**
/// over what the config wrote, so a value that itself spells `{line}` is
/// never read as a placeholder; and whether any was.
fn substituted(element: &[u8], values: &[(&str, &[u8], Value)]) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(element.len());
    let mut put = false;
    let mut i = 0;
    while i < element.len() {
        let hit = values
            .iter()
            .find(|(name, _, _)| element[i..].starts_with(name.as_bytes()));
        match hit {
            Some((name, v, _)) => {
                out.extend_from_slice(v);
                i += name.len();
                put = true;
            }
            None => {
                out.push(element[i]);
                i += 1;
            }
        }
    }
    (out, put)
}

/// **Whether a link's parts are free of a leading `-`**: what follows
/// `scheme:`, and an authority's user information, host and port each
/// (`[[source: rfc3986]]` § 3.2.1-3.2.3), spelled `-` or percent-encoded as
/// `%2D`, which a handler that decodes the URI reads as `-` too. A program
/// that takes the link apart and hands a part to another as an argument --
/// git handed `ssh` a host -- would hand it an option.
pub fn dashless(rest: &str) -> Result<(), Refused> {
    let dash =
        |s: &str| s.starts_with('-') || s.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("%2d"));
    if dash(rest) {
        return Err(Refused::Dash);
    }
    let Some(auth) = rest.strip_prefix("//") else {
        return Ok(());
    };
    let auth = &auth[..auth.find(['/', '?', '#']).unwrap_or(auth.len())];
    let (user, hostport) = match auth.rfind('@') {
        Some(at) => (Some(&auth[..at]), &auth[at + 1..]),
        None => (None, auth),
    };
    let (host, port) = match hostport.strip_prefix('[') {
        Some(v6) => match v6.find(']') {
            Some(close) => (&v6[..close], v6[close + 1..].strip_prefix(':')),
            None => (v6, None),
        },
        None => match hostport.rfind(':') {
            Some(c) => (&hostport[..c], Some(&hostport[c + 1..])),
            None => (hostport, None),
        },
    };
    let parts = user
        .into_iter()
        .flat_map(|u| u.split(':'))
        .chain([host])
        .chain(port);
    for p in parts {
        if dash(p) {
            return Err(Refused::Dash);
        }
    }
    Ok(())
}

/// **One `[link.NAME]` section as the file wrote it**: its name and its
/// keys, in order, gathered by `Config::apply` before any is read, so a rule
/// is read whole or refused whole.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Section {
    pub name: String,
    pub fields: Vec<(String, String)>,
}

impl Section {
    /// [`read`], for a test that holds its own sections: the source scan in
    /// `rules_are_read_only_from_the_config_file` counts `read`'s callers,
    /// and a test is not one.
    #[cfg(test)]
    pub fn read_all(
        sections: &[Section],
        home: Option<&Path>,
        path_var: Option<&OsStr>,
    ) -> (Rules, Vec<String>) {
        read(sections, home, path_var)
    }
}

/// **The rules in `sections`**, in file order, and what was refused, a
/// sentence each for the warning row. `home` makes a `~/` program absolute;
/// `path_var` is the `PATH` a command would run under, to say at read when
/// a program is not on it.
pub fn read(
    sections: &[Section],
    home: Option<&Path>,
    path_var: Option<&OsStr>,
) -> (Rules, Vec<String>) {
    let mut rules = Rules::NONE;
    let mut said = Vec::new();
    for s in sections {
        let at = format!("config: [link.{}]", s.name);
        if s.name == PATH_RULE {
            match path_rule(s, home) {
                Ok((forms, act)) => {
                    rules.forms = forms;
                    rules.path = act;
                    if let PathAct::Command(a) = &rules.path {
                        missing(&at, a, path_var, &mut said);
                    }
                }
                Err(why) => said.push(format!("{at} {why}; the rule is ignored")),
            }
            continue;
        }
        if rules.list.len() == MOST {
            said.push(format!(
                "{at} is past the {MOST} rules baud reads; the rule is ignored"
            ));
            continue;
        }
        match scheme_rule(s, home) {
            Ok(rule) => {
                if let Some((_, first)) = rules.rule_for(&rule.scheme) {
                    said.push(format!(
                        "{at} names {}:, which [link.{}] already takes; the rule is ignored",
                        rule.scheme, first.name
                    ));
                    continue;
                }
                if let Act::Command(a) = &rule.act {
                    missing(&at, a, path_var, &mut said);
                }
                if rule.suffix.is_some() {
                    rules.added = Added::Yes;
                }
                rules.list.push(rule);
            }
            Err(why) => said.push(format!("{at} {why}; the rule is ignored")),
        }
    }
    (rules, said)
}

/// Said at read when a rule's program is not there: the rule stays, as the
/// program may be installed later, and a click on its links is refused until
/// it is.
fn missing(at: &str, a: &Argv, path_var: Option<&OsStr>, said: &mut Vec<String>) {
    if a.found(path_var) == Program::Missing {
        let how = match a.program().contains('/') {
            true => "is not a program that can run",
            false => {
                "is not on PATH; a baud started from the Dock has the system's short PATH, \
                 so name it by its full path"
            }
        };
        said.push(format!("{at} command {:?} {how}", a.program()));
    }
}

/// A scalar value: quoted, or a bare word as baud's other keys take one.
fn scalar(v: &str) -> Result<&str, String> {
    let v = v.trim();
    let inner = match (v.strip_prefix('"'), v.starts_with('[')) {
        (Some(rest), _) => rest
            .strip_suffix('"')
            .filter(|i| !i.contains('"'))
            .ok_or_else(|| format!("{v} is not a single string"))?,
        (None, true) => return Err(format!("{v} is a list, not a value")),
        (None, false) => v,
    };
    match inner.is_empty() {
        true => Err("an empty value".to_string()),
        false => Ok(inner),
    }
}

/// **A list of quoted strings, read whole**, or why not: `[` then strings,
/// commas between and one allowed after the last, then `]`, and nothing
/// else. **A `]` inside a string ends a list written across lines** (the
/// reader gathers a list to the first line holding one), so a list cut there
/// does not end in its own `]`, and is refused rather than read short.
pub fn list(v: &str) -> Result<Vec<String>, String> {
    let v = v.trim();
    let cut = "a list written across lines ends at the first ] on a line, even inside a \
               string; write it on one line";
    let inner = v
        .strip_prefix('[')
        .ok_or_else(|| format!("{v} is not a list"))?
        .strip_suffix(']')
        .ok_or_else(|| cut.to_string())?;
    let mut out = Vec::new();
    let mut rest = inner.trim_start();
    while !rest.is_empty() {
        let body = rest
            .strip_prefix('"')
            .ok_or_else(|| format!("{v} holds something that is not a quoted string"))?;
        let end = body.find('"').ok_or_else(|| cut.to_string())?;
        out.push(body[..end].to_string());
        rest = body[end + 1..].trim_start();
        match rest.strip_prefix(',') {
            Some(r) => rest = r.trim_start(),
            None if rest.is_empty() => {}
            None => return Err(format!("{v} needs a comma between its strings")),
        }
    }
    Ok(out)
}

/// The keys of a section, each once and each one of `known`.
fn keys<'a>(
    s: &'a Section,
    known: &[&str],
) -> Result<impl Fn(&str) -> Option<&'a str> + 'a, String> {
    for (i, (k, _)) in s.fields.iter().enumerate() {
        if !known.contains(&k.as_str()) {
            return Err(format!("has no key {k:?}"));
        }
        if s.fields[..i].iter().any(|(j, _)| j == k) {
            return Err(format!("gives {k} twice"));
        }
    }
    Ok(move |key: &str| {
        s.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    })
}

/// A rule's name: what follows `link.`, of lowercase letters, digits, `_`
/// and `-`.
fn name_ok(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= SCHEME_MAX
        && n.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// **A scheme as RFC 3986 spells one** (§ 3.1), lowered, or why not.
pub fn scheme_ok(s: &str) -> Result<String, String> {
    let lower = s.to_ascii_lowercase();
    let mut b = lower.bytes();
    let first = b.next().is_some_and(|c| c.is_ascii_lowercase());
    let rest = b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"+-.".contains(&c));
    if !(first && rest) || lower.len() > SCHEME_MAX {
        return Err(format!(
            "scheme {s:?} is not a scheme: a letter, then letters, digits, +, - or ., \
             {SCHEME_MAX} at most, and no wildcard"
        ));
    }
    if REFUSED_SCHEMES.contains(&lower.as_str()) {
        return Err(format!(
            "scheme {s:?} carries code of its own, and no rule may open it"
        ));
    }
    Ok(lower)
}

/// The placeholders in `e`: each `{name}` with a name of lowercase letters.
fn placeholders(e: &str) -> impl Iterator<Item = &str> {
    e.match_indices('{').filter_map(move |(i, _)| {
        let rest = &e[i + 1..];
        let close = rest.find('}')?;
        let name = &rest[..close];
        (!name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase()))
            .then(|| &e[i..i + close + 2])
    })
}

/// **A command as the config wrote it, read**: a list of strings, the first
/// a program by its absolute path, under `~/`, or a name with no `/`; each
/// placeholder one of `allowed`, none in the program and `needed` among
/// them; none in a script ([`scripted`]); and no control in any element.
fn command(v: &str, allowed: &[&str], needed: &str, home: Option<&Path>) -> Result<Argv, String> {
    let mut argv = list(v).map_err(|e| format!("command: {e}"))?;
    let Some(prog) = argv.first().cloned() else {
        return Err("command names no program".to_string());
    };
    if prog.is_empty() {
        return Err("command names no program".to_string());
    }
    if placeholders(&prog).next().is_some() || prog.contains('{') {
        return Err("command's program is a placeholder; it must be written as it is".to_string());
    }
    for e in &argv {
        if e.chars().any(|c| c.is_control() || bidi_control(c)) {
            return Err("command holds a control character".to_string());
        }
        if let Some(p) = placeholders(e).find(|p| !allowed.contains(p)) {
            return Err(format!(
                "command names {p}, which this rule has no value for (it takes {})",
                allowed.join(", ")
            ));
        }
    }
    if !argv[1..].iter().any(|e| e.contains(needed)) {
        return Err(format!(
            "command never names {needed}, so a click would run the same thing whatever it was on"
        ));
    }
    if let Some(why) = scripted(&argv) {
        return Err(why);
    }
    argv[0] = match prog.strip_prefix("~/") {
        Some(rest) => match home {
            Some(h) => h.join(rest).to_string_lossy().into_owned(),
            None => return Err("command's program is under ~/, and baud has no home folder".into()),
        },
        None if prog.contains('/') && !prog.starts_with('/') => {
            return Err(format!(
                "command's program {prog:?} is a relative path; give its full path, or a name \
                 to find on PATH"
            ));
        }
        None => prog,
    };
    Ok(Argv(argv))
}

/// **A placeholder where a program would run it as code**, by
/// [`PROGRAMS`]' classes: in the element after a script flag of a shell or
/// interpreter named anywhere in the command -- `sh -c`, `bash -lc`, the
/// script flag of an interpreter baud knows (`-c`, `-C`, `-e`, `-E`, `-m`,
/// `-p` or `-r`, by interpreter), `osascript -e`, and an `env -S` that would
/// split it into arguments; as the first operand of `awk`, `sed`, `tclsh` or `Rscript`,
/// their program or script; anywhere after `deno` or `bun`; or in anything
/// after the destination of `ssh`, `mosh`, `autossh` or `et` (`kitten ssh`
/// is `ssh`), which goes to a shell on the other machine. Matched by the
/// element's file name, wherever it stands, so a wrapper (`env`, another
/// terminal's `-e`) does not hide one. The safe spelling passes the
/// link as an argument to a script that never contains it: a script file of
/// your own, `["/Users/you/bin/open-link", "{url}"]`, which reads it as `$1`.
fn scripted(argv: &[String]) -> Option<String> {
    let holds = |e: &String| placeholders(e).next().is_some();
    // Past the flags from `from`, each a short one taking an argument in
    // `values` skipping it too: the first operand, which may be past the end.
    let operand = |from: usize, values: &str| {
        let mut j = from;
        while let Some(f) = argv.get(j) {
            match f.strip_prefix('-').filter(|s| !s.is_empty()) {
                Some("-") => return j + 1,
                Some(flags) => {
                    let takes = flags.chars().last().is_some_and(|c| values.contains(c));
                    j += if takes && flags.len() == 1 { 2 } else { 1 };
                }
                None => return j,
            }
        }
        j
    };
    let script_flag = |f: &String, flags: &[char]| {
        f.strip_prefix('-')
            .filter(|s| {
                !s.starts_with('-') && !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic())
            })
            .is_some_and(|s| s.chars().any(|c| flags.contains(&c)))
    };
    for (i, e) in argv.iter().enumerate() {
        let name = program_of(e);
        if name == "env" {
            for (j, f) in argv.iter().enumerate().skip(i + 1) {
                let splits = ENV_SPLIT
                    .iter()
                    .any(|s| f == s || f.starts_with(&format!("{s}=")));
                if splits && (holds(f) || argv.get(j + 1).is_some_and(holds)) {
                    return Some("command splits a placeholder into arguments with env -S".into());
                }
            }
        }
        for (_, code) in PROGRAMS.iter().filter(|(n, _)| *n == name) {
            let script = || {
                Some(format!(
                    "command puts a placeholder in the script {name} runs; pass it as an \
                     argument to a script file of your own instead"
                ))
            };
            match *code {
                Code::Flag(flags) => {
                    for (j, f) in argv.iter().enumerate().skip(i + 1) {
                        let long = SCRIPT_LONG.iter().any(|l| f.eq_ignore_ascii_case(l));
                        let inline = SCRIPT_LONG
                            .iter()
                            .any(|l| f.to_ascii_lowercase().starts_with(&format!("{l}=")));
                        let next = argv.get(j + 1).is_some_and(holds);
                        if (inline && holds(f)) || ((script_flag(f, flags) || long) && next) {
                            return script();
                        }
                    }
                }
                Code::Operand { flags, values } => {
                    let at = operand(i + 1, values);
                    let mut given = false;
                    for (j, f) in argv.iter().enumerate().take(at).skip(i + 1) {
                        if script_flag(f, flags) {
                            given = true;
                            if argv.get(j + 1).is_some_and(holds) {
                                return script();
                            }
                        }
                    }
                    if !given && argv.get(at).is_some_and(holds) {
                        return script();
                    }
                }
                Code::Remote { values } => {
                    let at = operand(i + 1, values);
                    if argv.iter().skip(at + 1).any(holds) {
                        return Some(format!(
                            "command puts a placeholder after {name}'s destination, which {name} \
                             hands to a shell on the other machine"
                        ));
                    }
                }
                Code::Every => {
                    if argv.iter().skip(i + 1).any(holds) {
                        return Some(format!(
                            "command puts a placeholder where {name} may run it as code"
                        ));
                    }
                }
            }
        }
    }
    None
}

/// **A `[link.NAME]` rule, read whole**, or the first thing wrong with it.
/// The scheme defaults to the rule's name, and the action to `"command"`
/// when a command is named and `"open"` when none is, so each key read alone
/// is a whole rule.
fn scheme_rule(s: &Section, home: Option<&Path>) -> Result<Rule, String> {
    if !name_ok(&s.name) {
        return Err(
            "is not a rule's name: lowercase letters, digits, _ and -, 32 at most".to_string(),
        );
    }
    let get = keys(s, &RULE_KEYS)?;
    let scheme = scheme_ok(match get("scheme") {
        Some(v) => scalar(v).map_err(|e| format!("scheme: {e}"))?,
        None => &s.name,
    })?;
    let named = get("command");
    let act = match get("action")
        .map(scalar)
        .transpose()
        .map_err(|e| format!("action: {e}"))?
    {
        Some("open") | None if named.is_none() => Act::Open,
        Some("off") if named.is_none() => Act::Off,
        Some("copy") if named.is_none() => Act::Copy,
        Some("command") | None if named.is_some() => {
            Act::Command(command(named.unwrap_or_default(), &[URL], URL, home)?)
        }
        Some(a @ ("open" | "off" | "copy")) => {
            return Err(format!("names a command with action {a:?}"));
        }
        Some("command") => return Err("has action \"command\" and no command".to_string()),
        Some(other) => {
            return Err(format!(
                "action {other:?} is not open, command, copy or off"
            ));
        }
        None => unreachable!("both arms of a command named or not are above"),
    };
    if scheme == "file" && act == Act::Open {
        return Err(
            "opens file links; a file is shown in Finder and never opened by the system, \
             so name a command of yours to open one"
                .to_string(),
        );
    }
    let suffix = match crate::link::known_scheme(&scheme) {
        true => None,
        false => Some(Suffix::of(&scheme)),
    };
    Ok(Rule {
        name: s.name.clone(),
        scheme,
        act,
        suffix,
    })
}

/// `"on"` or `"off"`.
fn turned(key: &str, v: &str) -> Result<Turned, String> {
    match scalar(v).map_err(|e| format!("{key}: {e}"))? {
        "on" => Ok(Turned::On),
        "off" => Ok(Turned::Off),
        other => Err(format!("{key} {other:?} is not on or off")),
    }
}

/// **`[link.path]`, read whole**: the forms read, and what a click does.
fn path_rule(s: &Section, home: Option<&Path>) -> Result<(Forms, PathAct), String> {
    let get = keys(s, &PATH_KEYS)?;
    let mut forms = Forms::DEFAULT;
    if let Some(v) = get("forms") {
        let named = list(v).map_err(|e| format!("forms: {e}"))?;
        if let Some(bad) = named.iter().find(|n| !FORM_NAMES.contains(&n.as_str())) {
            return Err(format!(
                "forms names {bad:?}, which is not one of {}",
                FORM_NAMES.join(", ")
            ));
        }
        let on = |n: &str| match named.iter().any(|x| x == n) {
            true => Turned::On,
            false => Turned::Off,
        };
        forms.absolute = on("/");
        forms.home = on("~/");
        forms.dot = on("./");
        forms.parent = on("../");
        forms.bare = on("relative");
    }
    if let Some(v) = get("line_col") {
        forms.line_col = turned("line_col", v)?;
    }
    if let Some(v) = get("spaces") {
        forms.spaces = turned("spaces", v)?;
    }
    let named = get("command");
    let act = match get("action")
        .map(scalar)
        .transpose()
        .map_err(|e| format!("action: {e}"))?
    {
        Some("reveal") | None if named.is_none() => PathAct::Reveal,
        Some("off") if named.is_none() => PathAct::Off,
        Some("copy") if named.is_none() => PathAct::Copy,
        Some("command") | None if named.is_some() => PathAct::Command(command(
            named.unwrap_or_default(),
            &[PATH, LINE, COL],
            PATH,
            home,
        )?),
        Some(a @ ("reveal" | "off" | "copy")) => {
            return Err(format!("names a command with action {a:?}"));
        }
        Some("command") => return Err("has action \"command\" and no command".to_string()),
        Some(other) => {
            return Err(format!(
                "action {other:?} is not reveal, command, copy or off"
            ));
        }
        None => unreachable!("both arms of a command named or not are above"),
    };
    Ok((forms, act))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{Host, admit, admit_with};

    fn section(name: &str, fields: &[(&str, &str)]) -> Section {
        Section {
            name: name.to_string(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    fn rules(sections: &[Section]) -> (Rules, Vec<String>) {
        read(
            sections,
            Some(Path::new("/home/u")),
            Some(OsStr::new("/usr/bin:/bin")),
        )
    }

    fn args(c: &Command) -> Vec<String> {
        c.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    const NONE: Host<'static> = Host("");

    /// **E1: a rule with a key baud does not know is refused whole, and its
    /// command never runs**: `confirm = "always"`, a key a later baud might
    /// read as a safeguard, refuses the rule, so `ssh:` stays refused and no
    /// command exists for it. Perturbation: skip the unknown key and keep the
    /// rest, as the rest of the config does -- the rule is read and the
    /// command is built.
    #[test]
    fn an_unknown_key_refuses_the_rule_whole_and_runs_nothing() {
        let (r, said) = rules(&[section(
            "ssh",
            &[
                ("action", "\"command\""),
                ("command", "[\"/bin/sh\", \"{url}\"]"),
                ("confirm", "\"always\""),
            ],
        )]);
        assert!(r.list().is_empty(), "{r:?}");
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].contains("confirm") && said[0].contains("ignored"),
            "{said:?}"
        );
        assert_eq!(admit_with(b"ssh://h", NONE, &r), Err(Refused::Scheme));
        assert_eq!(r.command_found("ssh", None), None, "no command to run");
        // The control: without the unknown key the same rule is read.
        let (r, said) = rules(&[section(
            "ssh",
            &[
                ("action", "\"command\""),
                ("command", "[\"/bin/sh\", \"{url}\"]"),
            ],
        )]);
        assert_eq!(r.list().len(), 1, "{said:?}");
        let t = admit_with(b"ssh://h", NONE, &r).expect("admitted");
        assert_eq!(t.action(), crate::link::Action::Command);
    }

    /// **E1: an unknown value refuses the rule whole**: an action baud does
    /// not have for a scheme, `reveal` among them, `off` beside a command, a
    /// list that is not strings, a key given twice. Perturbation: fall back to `open` on an unknown action -- the
    /// first assertion fails.
    #[test]
    fn an_unknown_value_refuses_the_rule_whole() {
        for fields in [
            &[("action", "\"reveal\"")][..],
            &[("action", "\"open_anything\"")],
            &[
                ("action", "\"off\""),
                ("command", "[\"/bin/sh\", \"{url}\"]"),
            ],
            &[("scheme", "tel"), ("scheme", "sip")],
            &[("command", "[/bin/sh, \"{url}\"]")],
            &[("command", "\"/bin/sh {url}\"")],
            &[
                ("action", "\"open\""),
                ("command", "[\"/bin/sh\", \"{url}\"]"),
            ],
            &[("action", "\"command\"")],
        ] {
            let (r, said) = rules(&[section("tel", fields)]);
            assert!(r.list().is_empty(), "{fields:?}: {r:?}");
            assert_eq!(said.len(), 1, "{fields:?}: {said:?}");
        }
    }

    /// **Each key alone is a whole rule**: the scheme from the name, the
    /// action from whether a command is named -- what the example's
    /// commented sections uncomment to. Perturbation: require `action`.
    #[test]
    fn each_key_alone_is_a_whole_rule() {
        for fields in [&[("scheme", "\"tel\"")][..], &[("action", "\"open\"")], &[]] {
            let (r, said) = rules(&[section("tel", fields)]);
            assert_eq!(said, Vec::<String>::new(), "{fields:?}");
            assert_eq!(r.rule_for("tel").map(|(_, x)| x.act()), Some(&Act::Open));
        }
        let (r, said) = rules(&[section(
            "ssh",
            &[(
                "command",
                "[\"/bin/sh\", \"-c\", \"exec open $1\", \"sh\", \"{url}\"]",
            )],
        )]);
        assert_eq!(said, Vec::<String>::new());
        assert!(matches!(
            r.rule_for("ssh").map(|(_, x)| x.act()),
            Some(Act::Command(_))
        ));
    }

    /// **E2: no part of a link starts with `-`**, spelled or encoded, under
    /// any rule: CVE-2017-1000117's `ssh://-oProxyCommand=x@h`, a host, a
    /// port, a user's second part, and what follows the scheme's `:`. A link
    /// with a `-` inside a part opens. Perturbation: check the host only --
    /// the first link is admitted.
    #[test]
    fn no_part_of_a_link_starts_with_a_dash() {
        let (r, _) = rules(&[section("ssh", &[("command", "[\"/bin/sh\", \"{url}\"]")])]);
        for uri in [
            "ssh://-oProxyCommand=x@h",
            "ssh://-oProxyCommand=x",
            "ssh://u@-h",
            "ssh://u@h:-1",
            "ssh://u:-p@h",
            "ssh://%2DoProxyCommand=x@h",
            "ssh:-oProxyCommand=x",
            "ssh://[-1]",
        ] {
            assert_eq!(
                admit_with(uri.as_bytes(), NONE, &r),
                Err(Refused::Dash),
                "{uri}"
            );
        }
        for uri in ["ssh://user@host", "ssh://a-b@my-host:22/x-y", "ssh://h/-/x"] {
            assert!(admit_with(uri.as_bytes(), NONE, &r).is_ok(), "{uri}");
        }
        // A default's own link opens as it did without rules, even with a
        // part spelled so: the check is for what a rule routes.
        assert!(admit(b"https://-x.example/", NONE).is_ok());
        let (https, _) = rules(&[section("https", &[("command", "[\"/bin/sh\", \"{url}\"]")])]);
        assert_eq!(
            admit_with(b"https://-x.example/", NONE, &https),
            Err(Refused::Dash),
            "a command of yours gets no part that starts with -"
        );
    }

    /// **E2 after substitution: a value that starts with `-` is refused where
    /// it would land**, and a path's `{line}` that is not digits becomes `1`.
    /// Perturbation: drop the dash test in `landed`.
    #[test]
    fn a_substituted_value_never_starts_with_a_dash() {
        let (r, _) = rules(&[section(
            "path",
            &[("command", "[\"/bin/sh\", \"-g\", \"{path}:{line}:{col}\"]")],
        )]);
        let c = r
            .path_command(b"/a/b.rs", "12", "-3")
            .expect("a command")
            .expect("built");
        assert_eq!(args(&c), ["-g", "/a/b.rs:12:1"]);
        assert!(matches!(r.path_command(b"-rf", "", ""), Some(Err(_))));
        let (s, _) = rules(&[section("tel", &[("command", "[\"/bin/sh\", \"{url}\"]")])]);
        let (ix, _) = s.rule_for("tel").unwrap();
        assert!(matches!(s.command(ix, "-tel:1"), Some(Err(Refused::Dash))));
    }

    /// **E3: the command is an argv with no shell, its program literal, its
    /// standard streams the null device.** The link lands as one argument
    /// whatever it holds; a placeholder in `command[0]` is refused at read.
    /// Perturbation: build `sh -c` from the elements -- the program and
    /// argument assertions fail.
    #[test]
    fn a_command_is_an_argv_with_no_shell() {
        let (r, _) = rules(&[section(
            "tel",
            &[("command", "[\"/bin/sh\", \"--to\", \"{url}\"]")],
        )]);
        let (ix, _) = r.rule_for("tel").unwrap();
        let uri = "tel:1;$(touch${IFS}x)`y`";
        let c = r.command(ix, uri).unwrap().unwrap();
        assert_eq!(c.get_program(), "/bin/sh");
        assert_eq!(args(&c), ["--to", uri]);
        for prog in ["{url}", "x{url}", "{path}"] {
            let (r, said) = rules(&[section(
                "tel",
                &[("command", &format!("[\"{prog}\", \"{{url}}\"]"))],
            )]);
            assert!(r.list().is_empty(), "{prog}: {said:?}");
        }
        // A `~/` program is made absolute against the home folder, and a
        // relative one with a `/` is refused: a Dock launch's folder is `/`.
        let (r, _) = rules(&[section(
            "tel",
            &[("command", "[\"~/bin/dial\", \"{url}\"]")],
        )]);
        assert_eq!(
            r.rule_for("tel").map(|(_, x)| x.act()),
            Some(&Act::Command(Argv(vec![
                "/home/u/bin/dial".into(),
                "{url}".into()
            ])))
        );
        let (r, _) = rules(&[section("tel", &[("command", "[\"bin/dial\", \"{url}\"]")])]);
        assert!(r.list().is_empty());
    }

    /// **E3: the command's three standard streams are the null device**, so
    /// it holds no terminal and nothing it prints reaches one: the child
    /// itself compares each with `/dev/null` and leaves a mark only when all
    /// three are. Its reaping is the `Opener`'s, which both drivers keep and
    /// whose own test holds it to never waiting (`opener.rs`). The test
    /// waits on the child it spawned, so it is deterministic. Perturbation:
    /// inherit stdout -- no mark is left.
    #[test]
    fn a_command_has_null_streams() {
        let d = std::env::temp_dir().join(format!("baud-rules-null-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a scratch folder");
        let mark = d.join("null");
        let script = "if [ /dev/stdin -ef /dev/null ] && [ /dev/stdout -ef /dev/null ] && \
                      [ /dev/stderr -ef /dev/null ]; then : > $1; fi";
        let cmd = format!("[\"/bin/sh\", \"-c\", \"{script}\", \"sh\", \"{{path}}\"]");
        let (r, said) = rules(&[section("path", &[("command", &cmd)])]);
        assert_eq!(said, Vec::<String>::new());
        let mut c = r
            .path_command(mark.to_string_lossy().as_bytes(), "", "")
            .expect("a command")
            .expect("built");
        assert_eq!(c.get_program(), "/bin/sh");
        let status = c.spawn().expect("it starts").wait().expect("it ends");
        assert!(status.success());
        assert!(mark.exists(), "a stream was not the null device");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **E4: a placeholder in a shell's or interpreter's script is refused at
    /// read**, under a wrapper too, clustered flags included; passed as an
    /// argument to a script that never holds it, it is read. Perturbation:
    /// match `command[0]` only -- the `env` and `-lc` cases are read.
    #[test]
    fn a_placeholder_in_a_script_is_refused_at_read() {
        for cmd in [
            "[\"sh\", \"-c\", \"open {url}\"]",
            "[\"/bin/bash\", \"-lc\", \"open {url}\"]",
            "[\"/usr/bin/env\", \"zsh\", \"-c\", \"open {url}\"]",
            "[\"/usr/bin/env\", \"-S\", \"sh -c {url}\"]",
            "[\"python3\", \"-c\", \"import os; os.system('{url}')\"]",
            "[\"osascript\", \"-e\", \"open location {url}\"]",
            "[\"fish\", \"--command\", \"open {url}\"]",
            "[\"/usr/bin/ssh\", \"host\", \"open\", \"{url}\"]",
            "[\"/Applications/T.app/t\", \"-e\", \"sh\", \"-c\", \"{url}\"]",
        ] {
            let (r, said) = rules(&[section("tel", &[("command", cmd)])]);
            assert!(r.list().is_empty(), "{cmd}: read");
            assert_eq!(said.len(), 1, "{cmd}: {said:?}");
        }
        for cmd in [
            "[\"sh\", \"-c\", \"exec open $1\", \"sh\", \"{url}\"]",
            "[\"/usr/bin/ssh\", \"{url}\"]",
            "[\"/usr/bin/ssh\", \"-p\", \"22\", \"{url}\"]",
        ] {
            let (r, said) = rules(&[section("tel", &[("command", cmd)])]);
            assert_eq!(r.list().len(), 1, "{cmd}: {said:?}");
        }
    }

    /// Whether a rule running `cmd` is read: none said and one rule, or
    /// refused with one sentence.
    fn read_cmd(cmd: &str) -> bool {
        let (r, said) = rules(&[section("tel", &[("command", cmd)])]);
        assert!(said.len() <= 1, "{cmd}: {said:?}");
        !r.list().is_empty()
    }

    /// **E4, a versioned interpreter is the interpreter**: `python3.12`,
    /// `perl5.36`, `ruby3.3` and `lua5.4` by any path take a script after
    /// the same flags as their plain names, so a version in the name hides
    /// nothing; a name that only starts like one, `pythonista`, is not one.
    /// Perturbation: match the file name exactly -- every versioned case is
    /// read.
    #[test]
    fn a_versioned_interpreter_is_refused_as_its_name() {
        for cmd in [
            "[\"/usr/bin/python3.12\", \"-c\", \"{url}\"]",
            "[\"perl5.36\", \"-e\", \"{url}\"]",
            "[\"/opt/ruby3.3\", \"-e\", \"{url}\"]",
            "[\"lua5.4\", \"-e\", \"{url}\"]",
        ] {
            assert!(!read_cmd(cmd), "{cmd}: read");
        }
        assert!(
            read_cmd("[\"/usr/bin/pythonista\", \"-c\", \"{url}\"]"),
            "not an interpreter"
        );
        assert!(
            read_cmd("[\"/usr/bin/python3.12\", \"/my/open.py\", \"{url}\"]"),
            "an argument"
        );
    }

    /// **E4, the interpreters `bun`, `deno`, `Rscript` and `tclsh`**:
    /// `Rscript -e`, `bun -e` and `bun --eval` take a script after a flag;
    /// `deno` and `bun` run what follows a subcommand (`deno eval`, `deno
    /// run`, a module they fetch), so a placeholder anywhere after either is
    /// refused; `tclsh` and `Rscript` run their first operand as a script
    /// file. A placeholder after that operand is an argument, and is read.
    /// Perturbation: drop `deno` from the table -- `deno eval` is read.
    #[test]
    fn bun_deno_rscript_and_tclsh_take_no_placeholder_as_code() {
        for cmd in [
            "[\"Rscript\", \"-e\", \"{url}\"]",
            "[\"Rscript\", \"{path}\"]",
            "[\"bun\", \"-e\", \"{url}\"]",
            "[\"bun\", \"--eval\", \"{url}\"]",
            "[\"bun\", \"run\", \"{url}\"]",
            "[\"/usr/local/bin/deno\", \"eval\", \"{url}\"]",
            "[\"deno\", \"run\", \"{url}\"]",
            "[\"tclsh8.6\", \"{url}\"]",
        ] {
            assert!(!read_cmd(&cmd.replace("{path}", "{url}")), "{cmd}: read");
        }
        for cmd in [
            "[\"Rscript\", \"/my/open.R\", \"{url}\"]",
            "[\"tclsh\", \"/my/open.tcl\", \"{url}\"]",
        ] {
            assert!(read_cmd(cmd), "{cmd}: refused");
        }
    }

    /// **E4, a program text as an operand**: `awk`'s first operand is its
    /// program and `sed`'s its script, whose `e` command runs a shell, so a
    /// placeholder there, after `-e` or `-f`, or in `awk`'s past a `-F`
    /// value, is refused; with the program given by a flag the operands are
    /// input files, and are read. Perturbation: skip the value of `-F` as an
    /// operand -- the third case is read.
    #[test]
    fn awk_and_sed_take_no_placeholder_as_their_program() {
        for cmd in [
            "[\"awk\", \"{url}\"]",
            "[\"/usr/bin/gawk\", \"-v\", \"x=1\", \"{url}\"]",
            "[\"mawk\", \"-F\", \":\", \"{url}\"]",
            "[\"sed\", \"{url}\"]",
            "[\"sed\", \"-n\", \"-e\", \"{url}\"]",
            "[\"gsed\", \"-f\", \"{url}\"]",
        ] {
            assert!(!read_cmd(cmd), "{cmd}: read");
        }
        for cmd in [
            "[\"awk\", \"-f\", \"/my/p.awk\", \"{url}\"]",
            "[\"sed\", \"-n\", \"p\", \"{url}\"]",
        ] {
            assert!(read_cmd(cmd), "{cmd}: refused");
        }
    }

    /// **E4, remote shells**: `mosh`, `autossh`, `et`, and `kitten ssh`,
    /// like `ssh`, hand what follows the destination to a shell on the other
    /// machine, and `et -c` a command; a placeholder as the destination is
    /// read. Perturbation: treat `mosh` as any other program -- its case is
    /// read.
    #[test]
    fn remote_shells_take_no_placeholder_after_the_destination() {
        for cmd in [
            "[\"mosh\", \"host\", \"--\", \"open\", \"{url}\"]",
            "[\"mosh\", \"-p\", \"60001\", \"host\", \"{url}\"]",
            "[\"autossh\", \"-M\", \"0\", \"host\", \"{url}\"]",
            "[\"et\", \"host\", \"{url}\"]",
            "[\"et\", \"-c\", \"{url}\", \"host\"]",
            "[\"kitten\", \"ssh\", \"host\", \"{url}\"]",
        ] {
            assert!(!read_cmd(cmd), "{cmd}: read");
        }
        for cmd in [
            "[\"mosh\", \"{url}\"]",
            "[\"autossh\", \"-M\", \"0\", \"{url}\"]",
            "[\"kitten\", \"ssh\", \"{url}\"]",
        ] {
            assert!(read_cmd(cmd), "{cmd}: refused");
        }
    }

    /// **`off` turns a scheme off, one of baud's own included**: with
    /// `[link.mailto] action = "off"`, no mailto link is admitted, marked or
    /// printed -- `admit_with` is the gate for both, and the line at rest --
    /// and the bar says why; another scheme still opens. Perturbation: read
    /// `off` as `open` -- the mailto link is admitted.
    #[test]
    fn off_on_a_default_scheme_admits_nothing() {
        let (r, said) = rules(&[section("mailto", &[("action", "\"off\"")])]);
        assert_eq!(said, Vec::<String>::new());
        assert_eq!(admit_with(b"mailto:a@b", NONE, &r), Err(Refused::Off));
        assert_eq!(
            admit_with(b"MAILTO:a@b?subject=x", NONE, &r),
            Err(Refused::Off)
        );
        assert!(admit(b"mailto:a@b", NONE).is_ok(), "the control: no rule");
        assert!(
            admit_with(b"https://x/", NONE, &r).is_ok(),
            "another scheme"
        );
        assert_eq!(
            crate::link::refusal_words(Refused::Off),
            "off in your config"
        );
        assert_eq!(r.told(), ["config: [link.mailto] turns mailto: links off"]);
    }

    /// **`copy` puts the link's exact text on the clipboard and starts no
    /// process**: the target is the link as written, its action `Copy`, no
    /// command is built for it, and each driver's copy writes that text by
    /// the selection's own copy and starts nothing. The clipboard itself is
    /// not touched by a test. Perturbation: have a driver's `link_copied`
    /// start the opener -- the source assertion fails.
    #[test]
    fn copy_puts_the_exact_text_and_starts_no_process() {
        let (r, said) = rules(&[section("tel", &[("action", "\"copy\"")])]);
        assert_eq!(said, Vec::<String>::new());
        let uri = "tel:+1-555-0100;ext=7";
        let t = admit_with(uri.as_bytes(), NONE, &r).expect("admitted");
        assert_eq!((t.action(), t.as_str()), (crate::link::Action::Copy, uri));
        assert!(
            r.command(t.rule().expect("a rule's"), uri).is_none(),
            "no process"
        );
        for (name, src, write) in [
            (
                "app.rs",
                include_str!("app.rs"),
                "clipboard::write_text(&text)",
            ),
            (
                "linux.rs",
                include_str!("linux.rs"),
                "Board::Clipboard, &text)",
            ),
        ] {
            let (_, body) = src
                .split_once(concat!("fn link_", "copied(&mut self, text: String) {"))
                .expect(name);
            let (body, _) = body.split_once("\n    }\n").expect("its end");
            assert!(body.contains(write), "{name}: the write");
            // The words, said once the pasteboard took it: in the copy on
            // Linux, and on macOS where the turn's answer comes back.
            assert!(
                src.contains("self.selection_copied(&text);"),
                "{name}: the words"
            );
            assert!(!body.contains(".start("), "{name}: no process");
            assert!(
                src.contains("let text = t.as_str().to_string();\n"),
                "{name}: the link as written"
            );
        }
        let (p, said) = rules(&[section("path", &[("action", "\"copy\"")])]);
        assert_eq!(said, Vec::<String>::new());
        assert_eq!(p.path_act(), &PathAct::Copy);
        assert!(p.path_command(b"/a", "", "").is_none(), "no process");
    }

    /// **E5: bytes stay checked after substitution**: a link past `LINK_MAX`
    /// or with a byte outside printable ASCII, and a path with a C0, C1, DEL
    /// or bidi control, are refused where they would land. Perturbation:
    /// skip `landed` for paths -- the control cases build a command.
    #[test]
    fn bytes_stay_checked_after_substitution() {
        let (r, _) = rules(&[section("tel", &[("command", "[\"/bin/sh\", \"{url}\"]")])]);
        let (ix, _) = r.rule_for("tel").unwrap();
        let long = format!("tel:{}", "1".repeat(crate::link::MAX));
        assert!(matches!(r.command(ix, &long), Some(Err(Refused::Length))));
        assert!(matches!(r.command(ix, "tel:1 2"), Some(Err(Refused::Byte))));
        assert!(matches!(
            r.command(ix, "tel:\u{e9}"),
            Some(Err(Refused::Byte))
        ));
        let (p, _) = rules(&[section("path", &[("command", "[\"/bin/sh\", \"{path}\"]")])]);
        for bad in [
            "/a\u{1}b",
            "/a\u{7f}b",
            "/a\u{85}b",
            "/a\u{202e}b",
            "/a\u{2066}b",
        ] {
            assert!(
                matches!(
                    p.path_command(bad.as_bytes(), "", ""),
                    Some(Err(Refused::Byte))
                ),
                "{bad:?}"
            );
        }
        assert!(matches!(
            p.path_command(b"/a/\xff", "", ""),
            Some(Err(Refused::Byte))
        ));
        let ok = p
            .path_command("/a/\u{05e9}.txt".as_bytes(), "", "")
            .unwrap()
            .unwrap();
        assert_eq!(args(&ok), ["/a/\u{05e9}.txt"]);
    }

    /// **E6: no wildcard, no scheme outside RFC 3986's grammar, and none that
    /// carries its own code.** Perturbation: drop the refused list -- the
    /// `javascript` rule is read.
    #[test]
    fn schemes_with_code_or_wildcards_are_refused() {
        for s in [
            "*",
            "",
            "1tel",
            "+x",
            "t el",
            "t*",
            "javascript",
            "JavaScript",
            "data",
            "vbscript",
            "blob",
            "about",
            "a_b",
        ] {
            let (r, said) = rules(&[section("x", &[("scheme", &format!("\"{s}\""))])]);
            assert!(r.list().is_empty(), "{s:?} read");
            assert_eq!(said.len(), 1, "{s:?}: {said:?}");
        }
        for s in ["tel", "x-github-client", "git+ssh", "z39.50s", "VSCODE"] {
            let (r, said) = rules(&[section("x", &[("scheme", &format!("\"{s}\""))])]);
            assert_eq!(r.list().len(), 1, "{s:?}: {said:?}");
            assert_eq!(r.list()[0].scheme(), s.to_ascii_lowercase());
        }
        assert!(scheme_ok("javascript").is_err());
    }

    /// **E7: `file` stays local, `mailto` keeps only `subject` and `body`,
    /// whichever handler or command takes them**, and `file` is never opened
    /// by the system. Perturbation: skip the default's checks when a rule
    /// names its scheme -- the `bcc` and remote host links are admitted.
    #[test]
    fn the_defaults_own_rules_hold_under_a_rule() {
        let cmd = ("command", "[\"/bin/sh\", \"{url}\"]");
        let (r, _) = rules(&[section("mailto", &[cmd]), section("file", &[cmd])]);
        assert_eq!(
            admit_with(b"mailto:a@b?bcc=c@d", NONE, &r),
            Err(Refused::Header)
        );
        assert!(admit_with(b"mailto:a@b?subject=x", NONE, &r).is_ok());
        assert_eq!(
            admit_with(b"file://elsewhere/x", Host("here"), &r),
            Err(Refused::Host)
        );
        assert!(admit_with(b"file://here/x", Host("here"), &r).is_ok());
        let t = admit_with(b"file:///x", NONE, &r).unwrap();
        assert_eq!(t.action(), crate::link::Action::Command);
        let (open, said) = rules(&[section("file", &[("action", "\"open\"")])]);
        assert!(open.list().is_empty(), "{said:?}");
        assert_eq!(
            admit_with(b"file:///x", NONE, &open).unwrap().action(),
            crate::link::Action::Reveal
        );
    }

    /// **E11: a `]` inside a string ends a list written across lines**, and
    /// the cut list is refused, never read short. Perturbation: read the
    /// strings a cut list holds, as `strings()` does -- the rule is read with
    /// two elements.
    #[test]
    fn a_bracket_in_a_string_cuts_a_list_across_lines_and_is_refused() {
        let text = "[link.tel]\ncommand = [\n  \"/bin/sh\",\n  \"-x]y\",\n  \"{url}\",\n]\n";
        let mut c = crate::config::Config::defaults();
        let said = c.apply(text);
        assert!(
            c.link.rules.list().is_empty(),
            "read short: {:?}",
            c.link.rules
        );
        assert!(said.iter().any(|s| s.contains("one line")), "{said:?}");
        // The control: the same list on one line is read whole.
        let one = "[link.tel]\ncommand = [\"/bin/sh\", \"-x]y\", \"{url}\"]\n";
        let mut c = crate::config::Config::defaults();
        let _ = c.apply(one);
        let (ix, _) = c.link.rules.rule_for("tel").expect("read");
        let built = c.link.rules.command(ix, "tel:1").unwrap().unwrap();
        assert_eq!(args(&built), ["-x]y", "tel:1"]);
        assert_eq!(
            list("[\"a\", \"b\",]"),
            Ok(vec!["a".to_string(), "b".to_string()])
        );
        assert!(list("[ \"a\", \"x]").is_err());
    }

    /// **E12: a program not on `PATH` is said at read**, naming the Dock's
    /// short `PATH`, and the rule stays for when it is installed; a program
    /// on it says nothing. A scratch folder of this test's own is the `PATH`.
    /// Perturbation: drop `missing` -- the first assertion fails.
    #[test]
    fn a_program_not_on_path_is_said_at_read() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("baud-rules-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a scratch folder");
        let prog = d.join("dial");
        std::fs::write(&prog, "#!/bin/sh\n").expect("the program");
        std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).expect("its mode");
        let path_var = d.clone().into_os_string();
        let s = |p: &str| section("tel", &[("command", &format!("[\"{p}\", \"{{url}}\"]"))]);
        let (r, said) = read(&[s("nowhere-dial")], None, Some(&path_var));
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].contains("PATH") && said[0].contains("full path"),
            "{said:?}"
        );
        assert_eq!(r.list().len(), 1, "the rule stays");
        assert_eq!(
            r.command_found("tel", Some(&path_var)),
            Some(Program::Missing)
        );
        let (r, said) = read(&[s("dial")], None, Some(&path_var));
        assert_eq!(said, Vec::<String>::new());
        assert_eq!(
            r.command_found("tel", Some(&path_var)),
            Some(Program::Found)
        );
        assert_eq!(r.command_found("ssh", Some(&path_var)), None, "no rule");
        let p = section("path", &[("command", "[\"nowhere-edit\", \"{path}\"]")]);
        let (r, said) = read(&[p], None, Some(&path_var));
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(
            r.path_command_found(Some(&path_var)),
            Some(Program::Missing)
        );
        assert_eq!(Rules::NONE.path_command_found(Some(&path_var)), None);
        let abs = prog.to_string_lossy().into_owned();
        let (_, said) = read(&[s(&abs)], None, None);
        assert_eq!(said, Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **E13: your rules in file order, before baud's own**: the first rule
    /// naming a scheme takes it and a later one is refused and said; a rule
    /// naming `https` changes what a click runs, and the link is found as it
    /// was. Perturbation: let the last rule win -- the first assertion fails.
    #[test]
    fn rules_go_in_file_order_before_the_defaults() {
        let a = section(
            "a",
            &[
                ("scheme", "\"tel\""),
                ("command", "[\"/bin/sh\", \"{url}\"]"),
            ],
        );
        let b = section("b", &[("scheme", "\"tel\"")]);
        let (r, said) = rules(&[a, b]);
        assert_eq!(r.rule_for("tel").map(|(_, x)| x.name()), Some("a"));
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("[link.a] already takes"), "{said:?}");
        let (h, _) = rules(&[section("https", &[("command", "[\"/bin/sh\", \"{url}\"]")])]);
        assert_eq!(
            admit(b"https://x/", NONE).unwrap().action(),
            crate::link::Action::Open
        );
        assert_eq!(
            admit_with(b"https://x/", NONE, &h).unwrap().action(),
            crate::link::Action::Command
        );
        assert_eq!(
            h.list()[0].suffix,
            None,
            "a default's scheme is found by baud's own list"
        );
    }

    /// **`[link.path]` turns the forms on and off, and a bad one refuses it
    /// whole**, the defaults staying. Perturbation: keep the default forms
    /// whatever `forms` names -- `relative` stays off.
    #[test]
    fn the_path_rule_sets_the_forms() {
        let (r, said) = rules(&[section(
            "path",
            &[
                ("forms", "[\"/\", \"../\", \"relative\"]"),
                ("line_col", "\"on\""),
                ("spaces", "on"),
            ],
        )]);
        assert_eq!(said, Vec::<String>::new());
        let f = r.forms();
        assert_eq!(
            (
                f.absolute, f.home, f.dot, f.parent, f.bare, f.line_col, f.spaces
            ),
            (
                Turned::On,
                Turned::Off,
                Turned::Off,
                Turned::On,
                Turned::On,
                Turned::On,
                Turned::On
            )
        );
        assert_eq!(r.path_act(), &PathAct::Reveal);
        for bad in [
            &[("forms", "[\"bare\"]")][..],
            &[("line_col", "\"yes\"")],
            &[("action", "\"open\"")],
            &[("command", "[\"/bin/sh\", \"{url}\"]")],
            &[("command", "[\"/bin/sh\", \"{line}\"]")],
            &[("forms", "[\"/\"]"), ("bogus", "1")],
        ] {
            let (r, said) = rules(&[section("path", bad)]);
            assert_eq!(r.forms(), Forms::DEFAULT, "{bad:?}");
            assert_eq!(said.len(), 1, "{bad:?}: {said:?}");
        }
        let (r, _) = rules(&[section("path", &[("action", "\"reveal\"")])]);
        assert_eq!(r.path_act(), &PathAct::Reveal);
    }

    /// **The scan's prefilter never misses an added scheme**: every name, of
    /// one to thirty-two bytes, ends a tail it is the end of -- the longest
    /// by its last seven -- and with no added scheme nothing is compared.
    /// Perturbation: shift by the whole name's length -- an eight-byte name
    /// overflows the shift, and a long name never matches.
    #[test]
    fn the_prefilter_takes_every_added_scheme() {
        for name in ["t", "tel", "obsidian", "x-github-client", &"a".repeat(32)] {
            let (r, said) = rules(&[section("x", &[("scheme", &format!("\"{name}\""))])]);
            assert_eq!(said, Vec::<String>::new(), "{name}");
            let tail = name
                .bytes()
                .chain(*b":")
                .fold(0u64, |a, b| (a << 8) | u64::from(b));
            assert!(r.ends(Tail(tail)), "{name}");
            assert!(!r.ends(Tail(tail & !0xFF00)), "{name}: a byte changed");
        }
        assert!(!Rules::NONE.ends(Tail(u64::from_be_bytes(*b"\0\0\0\0tel:"))));
    }

    /// **E8: rules are read only from the config file**: [`read`] has one
    /// caller in every source file of the app, `Config::apply`, which reads
    /// the config text a load found and nothing a theme file, an escape
    /// sequence or the environment holds. Perturbation: call `read` from the
    /// palette, a driver or the terminal's input.
    #[test]
    fn rules_are_read_only_from_the_config_file() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let call = concat!("rules::", "read(");
        let mut callers = Vec::new();
        let mut files = 0;
        for e in std::fs::read_dir(&dir).expect("the source folder") {
            let p = e.expect("an entry").path();
            if p.extension().is_none_or(|x| x != "rs")
                || p.file_name() == Some(OsStr::new("rules.rs"))
            {
                continue;
            }
            files += 1;
            let text = std::fs::read_to_string(&p).expect("a source file");
            for _ in text.matches(call) {
                callers.push(p.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
        assert!(files > 50, "the control: the scan read {files} files");
        assert_eq!(callers, ["config.rs"], "one call, in config.rs");
        // The control: the call is in `apply`, which a load runs over the
        // file it found.
        let config = include_str!("config.rs");
        let (_, apply) = config.split_once("fn apply(").expect("apply");
        let (apply, _) = apply.split_once("\n    }\n").expect("its end");
        assert!(apply.contains(call), "the call is apply's");
    }
}
