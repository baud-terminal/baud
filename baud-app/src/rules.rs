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
//!   placeholder; a placeholder in the script a shell or an interpreter
//!   runs is refused at read ([`scripted`]), and so is one attached to a
//!   short option, or in a `--name=` option of a program that runs code
//!   ([`optioned`]). The list of programs that run their arguments as code
//!   cannot be complete, so the guide says why.
//! * **The defaults' own rules stay**: a `file` link names this machine or
//!   none, and a `mailto` carries only `subject` and `body`, whichever handler
//!   or command takes it (`link::admit_link`). A `file` link and a printed
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
use crate::paths::{Forms, NameLook, Turned};

/// The most rules read; the rest are refused and said. Each is a compare at
/// a `:` the frame's scan meets, so the number is kept small.
pub const MOST: usize = 32;

/// The longest scheme a rule may name.
pub const SCHEME_MAX: usize = 32;

/// **The keys of a `[link.NAME]` rule**, the one list the reader, the guide
/// and the example are held to.
pub const RULE_KEYS: [&str; 3] = ["scheme", "action", "command"];

/// **The keys of `[link.path]`**, the printed-path rule.
pub const PATH_KEYS: [&str; 6] = ["forms", "names", "line_col", "spaces", "action", "command"];

/// **The values of a `[link.NAME]` rule's `action`**, the one list the
/// reader's refusal, the guide and the example are held to.
pub const ACTIONS: [&str; 5] = ["open", "command", "window", "copy", "off"];

/// **The values of `[link.path]`'s `action`**.
pub const PATH_ACTIONS: [&str; 5] = ["reveal", "command", "window", "copy", "off"];

/// The name the printed-path rule is read under; no scheme rule takes it.
pub const PATH_RULE: &str = "path";

/// Schemes whose links carry code of their own, or a page the terminal
/// itself would show: never a rule's.
pub const REFUSED_SCHEMES: [&str; 5] = ["javascript", "data", "vbscript", "blob", "about"];

/// The printed-path forms `[link.path] forms` names, in [`Forms`]' order.
pub const FORM_NAMES: [&str; 7] = ["/", "~/", "./", "../", "relative", "name", "word"];

/// **The values of `[link.path]`'s `names`**, in [`NameLook`]'s order.
pub const NAME_LOOKS: [&str; 2] = ["line", "hover"];

/// The placeholder a scheme rule's command takes the link in.
pub const URL: &str = "{url}";
/// The placeholders `[link.path]`'s command takes: the path, and the
/// `:line:col` printed after it, `1` when none was.
pub const PATH: &str = "{path}";
pub const LINE: &str = "{line}";
pub const COL: &str = "{col}";

/// **A value a command takes**, by the placeholder that names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Placeholder {
    Url,
    Path,
    Line,
    Col,
}

impl Placeholder {
    /// Every placeholder, with its spelling.
    const ALL: [(Placeholder, &'static str); 4] = [
        (Placeholder::Url, URL),
        (Placeholder::Path, PATH),
        (Placeholder::Line, LINE),
        (Placeholder::Col, COL),
    ];
}

/// **A run of an argument**: text as the config wrote it, or a value.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Piece {
    Lit(String),
    Value(Placeholder),
}

/// **An argument of a command, read once**: text taken as it is, or pieces
/// with a value among them -- `{url}`, or `{path}:{line}:{col}` -- so a
/// click fills the values in and reads nothing the config wrote again, and
/// a value that itself spells a placeholder is never read as one.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Element {
    Literal(String),
    Pieces(Vec<Piece>),
}

impl Element {
    /// `e` split at each placeholder it names.
    fn read(e: &str) -> Element {
        let mut pieces = Vec::new();
        let mut lit = String::new();
        let mut rest = e;
        while let Some(c) = rest.chars().next() {
            match Placeholder::ALL.iter().find(|(_, s)| rest.starts_with(s)) {
                Some((p, s)) => {
                    if !lit.is_empty() {
                        pieces.push(Piece::Lit(core::mem::take(&mut lit)));
                    }
                    pieces.push(Piece::Value(*p));
                    rest = &rest[s.len()..];
                }
                None => {
                    lit.push(c);
                    rest = &rest[c.len_utf8()..];
                }
            }
        }
        if pieces.is_empty() {
            return Element::Literal(lit);
        }
        if !lit.is_empty() {
            pieces.push(Piece::Lit(lit));
        }
        Element::Pieces(pieces)
    }

    /// **What an argument that starts with `-` and takes a value is**, by
    /// its first piece: a long option, `--name=...`, or a short flag the
    /// value is attached to, `-X...`; none for any other.
    fn option(&self) -> Option<Opt> {
        match self {
            Element::Pieces(p) => match p.first() {
                Some(Piece::Lit(s)) if s.starts_with("--") => Some(Opt::Long),
                Some(Piece::Lit(s)) if s.starts_with('-') => Some(Opt::Short),
                Some(Piece::Lit(_) | Piece::Value(_)) | None => None,
            },
            Element::Literal(_) => None,
        }
    }

    /// The argument with each value put in, and whether any was.
    fn filled(&self, values: &[(Placeholder, &[u8], Value)]) -> Result<(Vec<u8>, Put), Refused> {
        match self {
            Element::Literal(s) => Ok((s.as_bytes().to_vec(), Put::No)),
            Element::Pieces(pieces) => {
                let mut out = Vec::new();
                for piece in pieces {
                    match piece {
                        Piece::Lit(s) => out.extend_from_slice(s.as_bytes()),
                        Piece::Value(p) => {
                            let (_, v, _) = values
                                .iter()
                                .find(|(q, _, _)| q == p)
                                .ok_or(Refused::Byte)?;
                            out.extend_from_slice(v);
                        }
                    }
                }
                Ok((out, Put::Yes))
            }
        }
    }
}

/// An argument that starts with `-` and takes a value ([`Element::option`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Opt {
    /// `--name=...`.
    Long,
    /// `-X...`, the value attached to a short flag.
    Short,
}

/// Whether a value went into an argument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Put {
    Yes,
    No,
}

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
    /// These flags split their argument into words, which would make a
    /// placeholder there a list of arguments, an option or a program among
    /// them: `env -S`.
    Split(&'static [&'static str]),
}

/// `ssh`'s flags that take an argument (`ssh(1)`'s synopsis).
const SSH_ARGS: &str = "BbcDEeFIiJLlmOoPpQRSWw";

/// **The programs baud knows run code, by file name**, the one list both
/// checks read: [`scripted`], for a placeholder in what they run, and
/// [`optioned`], which refuses a placeholder in a `--name=` option of a
/// command naming one. A name ending in a version, `python3.12` or
/// `perl5.36`, is read as the name before it ([`program_of`]). A program
/// may have more than one way in: `et` takes a command after `-c` and after
/// its destination. The list cannot be complete, and the guide says so.
const PROGRAMS: [(&str, Code); 38] = [
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
    ("env", Code::Split(&["-S", "--split-string"])),
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

/// What a rule does with a link a click opens.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Act {
    /// The system's handler for the scheme: LaunchServices on macOS,
    /// `xdg-open` on Linux.
    Open,
    /// Your program.
    Command(Argv),
    /// Your program, in a new baud window of its own: the same argv as a
    /// command, read by the same reader and refused for the same things, run
    /// in the window's terminal in place of its shell
    /// ([`Rules::window`]).
    Window(Argv),
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
    /// Your program, in a new baud window, its folder the file's own
    /// ([`Rules::path_window`]).
    Window(Argv),
    /// No printed path read at all, no lookup and no worker: what `[link]
    /// paths = "off"` does (`config::Link::paths_read`).
    Off,
    /// The path, as found, to the clipboard, after the click's check; nothing
    /// is opened.
    Copy,
}

/// **A command, as the config names it**: `command[0]` the program, by its
/// absolute path once read (`~/` taken against the home folder) or a name
/// looked up on `PATH` when it runs, never a placeholder; and the rest its
/// arguments, each read once into an [`Element`]. Minted only by
/// [`command`], which refuses a value in a script ([`scripted`]) or in an
/// option ([`optioned`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Argv {
    program: String,
    args: Vec<Element>,
}

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
        &self.program
    }

    /// The arguments as the config wrote them, each placeholder spelled.
    #[cfg(test)]
    pub fn written(&self) -> Vec<String> {
        let spelled = |p: Placeholder| {
            Placeholder::ALL
                .iter()
                .find(|(q, _)| *q == p)
                .map_or("", |(_, s)| s)
        };
        self.args
            .iter()
            .map(|e| match e {
                Element::Literal(s) => s.clone(),
                Element::Pieces(pieces) => pieces
                    .iter()
                    .map(|piece| match piece {
                        Piece::Lit(s) => s.as_str(),
                        Piece::Value(p) => spelled(*p),
                    })
                    .collect(),
            })
            .collect()
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

/// **The argv a new window runs in place of its shell**: a rule's
/// [`Argv`] with its placeholders replaced by the link or the path and
/// checked where they landed, as a command's are ([`landed`]), built once
/// per click and moved into the window's start. No shell is between: the
/// first element is the program, the rest its arguments, each one argument
/// whatever it holds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Run(Vec<std::ffi::OsString>);

impl Run {
    /// **An argv given whole**, `baud -e`'s: none when it names no program.
    /// Its elements are taken as they are, never split or joined.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn given(argv: Vec<std::ffi::OsString>) -> Option<Run> {
        match argv.first().is_some_and(|p| !p.is_empty()) {
            true => Some(Run(argv)),
            false => None,
        }
    }

    /// The program, as the rule wrote it or made absolute.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn program(&self) -> &OsStr {
        &self.0[0]
    }

    /// **Whether the program is there to run**, on `path_var`'s folders for a
    /// bare name, as [`Argv::found`] asks of a rule: asked where a window
    /// starts, before anything opens, so a program that is gone opens no
    /// window. A metadata call: never on the frame path. A name that is
    /// not UTF-8 is never one a rule wrote, and is not looked for.
    pub fn found(&self, path_var: Option<&OsStr>) -> Program {
        match self.program().to_str() {
            Some(p) => program_found(p, path_var),
            None => Program::Missing,
        }
    }

    /// Every element, the program first.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn argv(&self) -> &[std::ffi::OsString] {
        &self.0
    }

    /// The elements, moved out, the program first.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn into_argv(self) -> Vec<std::ffi::OsString> {
        self.0
    }
}

/// **What a rule runs a click with**, the two actions that run your program:
/// a command, with no terminal, or a window, in a terminal of its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Runs {
    Command,
    Window,
}

impl Act {
    /// The argv a click runs, and how, for the two actions that run one.
    fn runs(&self) -> Option<(&Argv, Runs)> {
        match self {
            Act::Command(a) => Some((a, Runs::Command)),
            Act::Window(a) => Some((a, Runs::Window)),
            Act::Open | Act::Off | Act::Copy => None,
        }
    }
}

impl PathAct {
    /// [`Act::runs`] for a printed path.
    fn runs(&self) -> Option<(&Argv, Runs)> {
        match self {
            PathAct::Command(a) => Some((a, Runs::Command)),
            PathAct::Window(a) => Some((a, Runs::Window)),
            PathAct::Reveal | PathAct::Off | PathAct::Copy => None,
        }
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

    /// **The programs your `[link.NAME]` rules run**: each `command` or
    /// `window` rule's `command[0]`, as [`Argv::program`] spells it, in file
    /// order, for the worker to look up. `[link.path]`'s is not among them.
    pub fn command_programs(&self) -> impl Iterator<Item = &str> {
        self.list
            .iter()
            .filter_map(|r| r.act().runs().map(|(argv, _)| argv.program()))
    }

    /// **The program a click on scheme `name`'s links would run, and whether
    /// it is there** -- none where no rule of yours runs a command or a
    /// window for it. The strike's question: a metadata call, so never on the
    /// frame path, and asked by the worker that probes paths, which no driver
    /// has wired to it yet.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn command_found(&self, name: &str, path_var: Option<&OsStr>) -> Option<Program> {
        let (argv, _) = self.rule_for(name)?.1.act().runs()?;
        Some(argv.found(path_var))
    }

    /// [`Rules::command_found`] for a click on a printed path.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn path_command_found(&self, path_var: Option<&OsStr>) -> Option<Program> {
        let (argv, _) = self.path.runs()?;
        Some(argv.found(path_var))
    }

    /// **The command a click on `uri`, admitted under rule `ix`, runs**:
    /// `{url}` replaced by the whole link, no shell, the standard streams the
    /// null device. The link's bytes are checked again where they land
    /// ([`landed`]); none if the rule runs no command.
    pub fn command(&self, ix: RuleIx, uri: &str) -> Option<Result<Command, Refused>> {
        let (argv, Runs::Command) = self.at(ix)?.act().runs()? else {
            return None;
        };
        Some(filled(argv, &[(Placeholder::Url, uri.as_bytes(), Value::Uri)]).map(command_of))
    }

    /// **The argv a new window runs for a click on `uri`, admitted under rule
    /// `ix`**: [`Rules::command`]'s substitution and checks, the same bytes
    /// refused for the same reasons, handed to the window's terminal rather
    /// than to a process with no terminal; none if the rule opens no window.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn window(&self, ix: RuleIx, uri: &str) -> Option<Result<Run, Refused>> {
        let (argv, Runs::Window) = self.at(ix)?.act().runs()? else {
            return None;
        };
        Some(filled(
            argv,
            &[(Placeholder::Url, uri.as_bytes(), Value::Uri)],
        ))
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
        let (argv, Runs::Command) = self.path.runs()? else {
            return None;
        };
        Some(path_filled(argv, path, line, col).map(command_of))
    }

    /// **The argv a new window runs for a click on printed path `path`**,
    /// as [`Rules::path_command`] builds a command's; none when a click does
    /// not open a window.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn path_window(&self, path: &[u8], line: &str, col: &str) -> Option<Result<Run, Refused>> {
        let (argv, Runs::Window) = self.path.runs()? else {
            return None;
        };
        Some(path_filled(argv, path, line, col))
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
                Act::Window(a) => format!(
                    "config: [link.{}] opens {}: links from program output in a new window \
                     running {}",
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
            PathAct::Window(a) => out.push(format!(
                "config: [link.path] opens a clicked printed path in a new window running {}",
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

/// `run` as a [`Command`] with no shell and null standard streams: what a
/// rule's command starts, with no terminal.
fn command_of(run: Run) -> Command {
    let mut argv = run.into_argv().into_iter();
    // `filled` never returns an empty argv: the reader refused a command
    // naming no program.
    let mut c = Command::new(argv.next().unwrap_or_default());
    c.args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

/// `argv` filled for printed path `path`, `line` and `col` the digits
/// printed after it, `1` where none were; a path that is not absolute is
/// refused, as the reader admits no other.
fn path_filled(argv: &Argv, path: &[u8], line: &str, col: &str) -> Result<Run, Refused> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let line = if digits(line) { line } else { "1" };
    let col = if digits(col) { col } else { "1" };
    if path.first() != Some(&b'/') {
        return Err(Refused::Host);
    }
    filled(
        argv,
        &[
            (Placeholder::Path, path, Value::Path),
            (Placeholder::Line, line.as_bytes(), Value::Uri),
            (Placeholder::Col, col.as_bytes(), Value::Uri),
        ],
    )
}

/// `argv` with each value put in where its placeholder was read, checked
/// where it landed: the one substitution a command and a window share.
fn filled(argv: &Argv, values: &[(Placeholder, &[u8], Value)]) -> Result<Run, Refused> {
    use std::os::unix::ffi::OsStringExt;
    for (_, v, what) in values {
        landed(v, *what)?;
    }
    let mut out: Vec<std::ffi::OsString> = Vec::with_capacity(1 + argv.args.len());
    out.push(argv.program.as_str().into());
    for e in &argv.args {
        let (bytes, put) = e.filled(values)?;
        // An element is checked whole once a value is in it: nothing the
        // config wrote can hold a control either, but this is the bytes that
        // are handed over.
        if put == Put::Yes && landed_element(&bytes).is_err() {
            return Err(Refused::Byte);
        }
        out.push(std::ffi::OsString::from_vec(bytes));
    }
    Ok(Run(out))
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
                    if let Some((a, _)) = rules.path.runs() {
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
                if let Some((a, _)) = rule.act.runs() {
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
/// them; none in a script ([`scripted`]) or an option ([`optioned`]); and
/// no control in any element.
fn command(v: &str, allowed: &[&str], needed: &str, home: Option<&Path>) -> Result<Argv, String> {
    let argv = list(v).map_err(|e| format!("command: {e}"))?;
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
    let args: Vec<Element> = argv[1..].iter().map(|e| Element::read(e)).collect();
    if let Some(why) = optioned(&argv, &args) {
        return Err(why);
    }
    let program = match prog.strip_prefix("~/") {
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
    Ok(Argv { program, args })
}

/// **A placeholder in an option**: an argument that starts with `-` and
/// takes a value. A short flag the value is attached to, `-c{url}` or
/// `-Ssh -c {url}`, is refused for every program, since a program reading
/// its options with getopt takes the value as the next argument too, so the
/// two can always be written apart. A long option, `--name={url}`, is
/// refused when the command names a program [`PROGRAMS`] knows runs code,
/// wherever it stands, and taken for any other: a browser's `--app={url}`
/// has no form with the value apart. A program that runs code from a
/// `--name=` option and is not in the list is not caught, and the guide
/// says so.
fn optioned(argv: &[String], args: &[Element]) -> Option<String> {
    let runs_code = argv.iter().find_map(|e| {
        let name = program_of(e);
        PROGRAMS.iter().any(|(n, _)| *n == name).then_some(name)
    });
    for (e, written) in args.iter().zip(&argv[1..]) {
        match (e.option(), runs_code) {
            (Some(Opt::Short), _) => {
                return Some(format!(
                    "command attaches a placeholder to the option {written:?}; write the option \
                     and its placeholder as two elements"
                ));
            }
            (Some(Opt::Long), Some(name)) => {
                return Some(format!(
                    "command puts a placeholder in the option {written:?} of {name}, which runs \
                     code; write the option and its placeholder as two elements, or pass it to \
                     a script of your own"
                ));
            }
            (Some(Opt::Long), None) | (None, _) => {}
        }
    }
    None
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
                Code::Split(splits) => {
                    for (j, f) in argv.iter().enumerate().skip(i + 1) {
                        let split = splits
                            .iter()
                            .any(|s| f == s || f.starts_with(&format!("{s}=")));
                        if split && (holds(f) || argv.get(j + 1).is_some_and(holds)) {
                            return Some(format!(
                                "command splits a placeholder into arguments with {name} {f}"
                            ));
                        }
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
        Some("window") if named.is_some() => {
            Act::Window(command(named.unwrap_or_default(), &[URL], URL, home)?)
        }
        Some(a @ ("open" | "off" | "copy")) => {
            return Err(format!("names a command with action {a:?}"));
        }
        Some(a @ ("command" | "window")) => {
            return Err(format!("has action {a:?} and no command"));
        }
        Some(other) => {
            return Err(format!("action {other:?} is not {}", one_of(&ACTIONS)));
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

/// `a, b, c or d`, for a sentence naming the values a key takes.
fn one_of(values: &[&str]) -> String {
    match values.split_last() {
        Some((last, [])) => (*last).to_string(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
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
        forms.name = on("name");
        forms.word = on("word");
    }
    if let Some(v) = get("names") {
        forms.names = match scalar(v).map_err(|e| format!("names: {e}"))? {
            "line" => NameLook::Line,
            "hover" => NameLook::Hover,
            other => {
                return Err(format!("names {other:?} is not {}", one_of(&NAME_LOOKS)));
            }
        };
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
        Some("window") if named.is_some() => PathAct::Window(command(
            named.unwrap_or_default(),
            &[PATH, LINE, COL],
            PATH,
            home,
        )?),
        Some(a @ ("reveal" | "off" | "copy")) => {
            return Err(format!("names a command with action {a:?}"));
        }
        Some(a @ ("command" | "window")) => {
            return Err(format!("has action {a:?} and no command"));
        }
        Some(other) => {
            return Err(format!("action {other:?} is not {}", one_of(&PATH_ACTIONS)));
        }
        None => unreachable!("both arms of a command named or not are above"),
    };
    Ok((forms, act))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{MachineNames, Target, admit};

    /// A link read off a local program's line, judged with `rules`.
    fn admit_with<'a>(
        uri: &'a [u8],
        host: &MachineNames,
        rules: &Rules,
    ) -> Result<Target<'a>, Refused> {
        crate::link::admit_link(
            uri,
            host,
            rules,
            crate::paths::LinkOrigin::given(crate::paths::Origin::Local),
        )
    }

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

    const NONE: &MachineNames = &MachineNames::NONE;

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
            Some(&Act::Command(Argv {
                program: "/home/u/bin/dial".into(),
                args: vec![Element::Pieces(vec![Piece::Value(Placeholder::Url)])],
            }))
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
        // macOS asks its effect queue, whose turn writes the pasteboard.
        assert!(
            include_str!("os_effect.rs").contains("clipboard::write_text(t, text.as_str())"),
            "the macOS write"
        );
        for (name, src, write, said) in [
            (
                "app.rs",
                include_str!("app.rs"),
                "from: crate::os_effect::CopyFrom::Link,",
                "self.selection_copied(text.as_str());",
            ),
            (
                "linux.rs",
                include_str!("linux.rs"),
                "Board::Clipboard, &text)",
                "self.selection_copied(&text);",
            ),
        ] {
            let (_, body) = src
                .split_once(concat!("fn link_", "copied(&mut self, text: String) {"))
                .expect(name);
            let (body, _) = body.split_once("\n    }\n").expect("its end");
            assert!(body.contains(write), "{name}: the write");
            // The words, said once the pasteboard took it: in the copy on
            // Linux, and on macOS where the turn's answer comes back.
            assert!(src.contains(said), "{name}: the words");
            assert!(!body.contains(".start("), "{name}: no process");
            // The app's copy is the shared decision's (`link_layer::decide`).
            let written = match name {
                "app.rs" => {
                    src.contains(concat!("Then::Copy(text) => ", "self.link_copied(text)"))
                        && include_str!("link_layer.rs")
                            .contains(concat!("Then::Copy(t.as_str()", ".to_string())"))
                }
                _ => src.contains("let text = t.as_str().to_string();\n"),
            };
            assert!(written, "{name}: the link as written");
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
            admit_with(
                b"file://elsewhere/x",
                &crate::link::MachineNames::one("here"),
                &r
            ),
            Err(Refused::Host)
        );
        assert!(
            admit_with(
                b"file://here/x",
                &crate::link::MachineNames::one("here"),
                &r
            )
            .is_ok()
        );
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
    /// whatever `forms` names -- `~/` stays on.
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
            [
                f.absolute, f.home, f.dot, f.parent, f.bare, f.name, f.word, f.line_col, f.spaces
            ],
            [
                Turned::On,
                Turned::Off,
                Turned::Off,
                Turned::On,
                Turned::On,
                Turned::Off,
                Turned::Off,
                Turned::On,
                Turned::On
            ]
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

    /// **Single names: read by default, read when `forms` lists `"name"`
    /// and gone when it leaves it out, `"word"` read when named, and
    /// `names` sets their look, an unknown value ignoring the section with
    /// a warning.** Named breaks: default names off -- the first assertion;
    /// keep `name` on whatever `forms` names -- the list without it still
    /// reads names; read `names` as `"line"` whatever it says -- `"hover"`
    /// is lost.
    #[test]
    fn the_path_rule_sets_the_names() {
        assert_eq!(
            (Forms::DEFAULT.name, Forms::DEFAULT.word),
            (Turned::On, Turned::Off),
            "names by default, words not"
        );
        assert_eq!(Forms::DEFAULT.names, NameLook::Line);
        let (r, said) = rules(&[section(
            "path",
            &[("forms", "[\"/\", \"~/\", \"./\", \"name\"]")],
        )]);
        assert_eq!(said, Vec::<String>::new());
        assert_eq!(r.forms().name, Turned::On, "listed: read");
        let (r, said) = rules(&[section("path", &[("forms", "[\"/\", \"~/\", \"./\"]")])]);
        assert_eq!(said, Vec::<String>::new());
        assert_eq!(r.forms().name, Turned::Off, "left out: none read");
        let (r, said) = rules(&[section(
            "path",
            &[("forms", "[\"name\", \"word\"]"), ("names", "\"hover\"")],
        )]);
        assert_eq!(said, Vec::<String>::new());
        let f = r.forms();
        assert_eq!(
            (f.absolute, f.name, f.word, f.names),
            (Turned::Off, Turned::On, Turned::On, NameLook::Hover)
        );
        let (r, said) = rules(&[section("path", &[("names", "\"line\"")])]);
        assert_eq!((said.len(), r.forms().names), (0, NameLook::Line));
        for bad in ["\"underline\"", "\"Hover\"", "on"] {
            let (r, said) = rules(&[section("path", &[("forms", "[\"/\"]"), ("names", bad)])]);
            assert_eq!(r.forms(), Forms::DEFAULT, "{bad}: the section ignored");
            assert_eq!(said.len(), 1, "{bad}: one warning, {said:?}");
            assert!(said[0].contains("names"), "{said:?}");
        }
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
        let (_, apply) = config.split_once("fn apply_in(").expect("apply_in");
        let (apply, _) = apply.split_once("\n    }\n").expect("its end");
        assert!(apply.contains(call), "the call is apply's");
    }

    /// Commands the command reader refuses, one reason each, `{url}`
    /// standing for the rule's own placeholder: a script of a shell or an
    /// interpreter, under a wrapper and versioned; an operand that is a
    /// program; what follows a remote shell's destination; a program that is
    /// a placeholder or a relative path; a control; a placeholder the rule
    /// has no value for; none at all; and what is not a list of strings.
    const REFUSED_COMMANDS: [&str; 16] = [
        "[\"sh\", \"-c\", \"open {url}\"]",
        "[\"/usr/bin/env\", \"zsh\", \"-c\", \"open {url}\"]",
        "[\"/usr/bin/env\", \"-S\", \"sh -c {url}\"]",
        "[\"osascript\", \"-e\", \"open location {url}\"]",
        "[\"/usr/bin/python3.12\", \"-c\", \"{url}\"]",
        "[\"deno\", \"run\", \"{url}\"]",
        "[\"awk\", \"{url}\"]",
        "[\"/usr/bin/ssh\", \"host\", \"open\", \"{url}\"]",
        "[\"{url}\", \"{url}\"]",
        "[\"bin/w3m\", \"{url}\"]",
        "[\"w3m\", \"\u{1b}\", \"{url}\"]",
        "[\"w3m\", \"{nope}\", \"{url}\"]",
        "[\"w3m\", \"-dump\"]",
        "[]",
        "\"w3m {url}\"",
        "[w3m, \"{url}\"]",
    ];

    /// Commands the reader reads.
    const READ_COMMANDS: [&str; 4] = [
        "[\"w3m\", \"{url}\"]",
        "[\"~/bin/open-link\", \"{url}\"]",
        "[\"/usr/bin/env\", \"w3m\", \"-o\", \"x=1\", \"{url}\"]",
        "[\"sh\", \"-c\", \"exec w3m $1\", \"sh\", \"{url}\"]",
    ];

    /// **W1: `action = "window"` reads its command through a command's own
    /// reader**, on a scheme rule and on `[link.path]`: every command a
    /// `command` rule refuses, a `window` rule refuses with the same
    /// sentence, and every command one reads the other reads to the same
    /// argv, its program made absolute the same. Perturbation: read a
    /// window's command with `list` alone, skipping `command`'s checks --
    /// every refused case is read.
    #[test]
    fn a_window_reads_its_command_as_a_command_does() {
        let (control, said) = rules(&[section(
            "tel",
            &[
                ("action", "\"window\""),
                ("command", "[\"/bin/sh\", \"{url}\"]"),
            ],
        )]);
        assert_eq!(said, Vec::<String>::new());
        assert!(
            matches!(
                control.rule_for("tel").map(|(_, r)| r.act()),
                Some(Act::Window(_))
            ),
            "the control: a window rule is read"
        );
        for (name, holder) in [("tel", URL), (PATH_RULE, PATH)] {
            let read_as = |action: &str, cmd: &str| {
                let cmd = cmd.replace(URL, holder);
                rules(&[section(
                    name,
                    &[("action", &format!("\"{action}\"")), ("command", &cmd)],
                )])
            };
            for cmd in REFUSED_COMMANDS {
                let (c, c_said) = read_as("command", cmd);
                let (w, w_said) = read_as("window", cmd);
                assert_eq!(c_said.len(), 1, "[link.{name}] command {cmd}: {c_said:?}");
                assert_eq!(w_said, c_said, "[link.{name}] window {cmd}");
                assert_eq!(
                    (c, w),
                    (Rules::NONE, Rules::NONE),
                    "[link.{name}] {cmd}: read"
                );
            }
            for cmd in READ_COMMANDS {
                let (c, c_said) = read_as("command", cmd);
                let (w, w_said) = read_as("window", cmd);
                assert_eq!(w_said, c_said, "[link.{name}] {cmd}: said the same");
                assert!(
                    c != Rules::NONE && w != Rules::NONE,
                    "[link.{name}] {cmd}: refused"
                );
                let (a, b) = match name {
                    PATH_RULE => match (c.path_act(), w.path_act()) {
                        (PathAct::Command(a), PathAct::Window(b)) => (a.clone(), b.clone()),
                        other => panic!("[link.path] {cmd}: {other:?}"),
                    },
                    _ => match (c.list()[0].act(), w.list()[0].act()) {
                        (Act::Command(a), Act::Window(b)) => (a.clone(), b.clone()),
                        other => panic!("[link.{name}] {cmd}: {other:?}"),
                    },
                };
                assert_eq!(a, b, "[link.{name}] {cmd}: the same argv");
            }
        }
    }

    /// **W2: a window's argv is filled as a command's is**: `{url}`, and
    /// `{path}`, `{line}` and `{col}`, each replaced in one pass, the program
    /// literal and no shell between, the same elements a command rule would
    /// hand its process; a dashed link, a control byte and a path that is
    /// not absolute refused as a command's are; and neither builder answers
    /// for the other's action. Perturbation: return the rule's argv unfilled
    /// from `Rules::window` -- the first assertion fails.
    #[test]
    fn a_window_fills_its_argv_as_a_command_does() {
        let w = |s: &str, cmd: &str| {
            rules(&[section(s, &[("action", "\"window\""), ("command", cmd)])]).0
        };
        let c = |s: &str, cmd: &str| rules(&[section(s, &[("command", cmd)])]).0;
        let uri = "https://github.com/x/y?a=1&b=$(z)";
        let https = w("https", "[\"w3m\", \"{url}\"]");
        let (ix, _) = https.rule_for("https").expect("the rule");
        let run = https.window(ix, uri).expect("a window").expect("built");
        assert_eq!(run.argv(), ["w3m", uri]);
        assert_eq!(run.program(), "w3m");
        let cmd = c("https", "[\"w3m\", \"{url}\"]");
        let (cix, _) = cmd.rule_for("https").expect("the rule");
        let built = cmd.command(cix, uri).expect("a command").expect("built");
        assert_eq!(
            core::iter::once(built.get_program())
                .chain(built.get_args())
                .collect::<Vec<_>>(),
            run.argv(),
            "the elements a command would run"
        );
        assert!(
            https.command(ix, uri).is_none(),
            "a window rule builds no command"
        );
        assert!(
            cmd.window(cix, uri).is_none(),
            "a command rule builds no window"
        );
        assert_eq!(https.window(ix, "-x"), Some(Err(Refused::Dash)));
        assert_eq!(
            https.window(ix, "https://x/\u{7f}"),
            Some(Err(Refused::Byte))
        );

        let path = w(PATH_RULE, "[\"hx\", \"{path}:{line}:{col}\"]");
        let run = path
            .path_window(b"/tmp/a b;$x.rs", "12", "")
            .expect("a window");
        assert_eq!(run.expect("built").argv(), ["hx", "/tmp/a b;$x.rs:12:1"]);
        assert!(path.path_command(b"/tmp/a", "", "").is_none(), "no command");
        assert!(
            c(PATH_RULE, "[\"hx\", \"{path}\"]")
                .path_window(b"/tmp/a", "", "")
                .is_none(),
            "a command builds no window"
        );
        assert_eq!(path.path_window(b"rel/a", "", ""), Some(Err(Refused::Host)));
        assert_eq!(
            path.path_window(b"/a\x01", "", ""),
            Some(Err(Refused::Byte))
        );
        assert_eq!(
            path.path_window(b"/a\xff", "", ""),
            Some(Err(Refused::Byte))
        );
    }

    /// **W3: a window rule is a whole rule, admitted, looked up, struck and
    /// said as a command's is**: `"window"` with no command is refused and
    /// said; an unknown action's sentence names every value, `window`
    /// among them; a window rule's link is admitted as `Action::Window`, its
    /// program is one the worker looks up and the strike asks about, a
    /// missing one is said at read, and the load says what it does.
    /// Perturbation: leave `Act::Window` out of `Act::runs` -- the program is
    /// never looked up and nothing is said for a missing one.
    #[test]
    fn a_window_rule_is_admitted_looked_up_and_said() {
        for name in ["tel", PATH_RULE] {
            let (r, said) = rules(&[section(name, &[("action", "\"window\"")])]);
            assert_eq!(r, Rules::NONE, "[link.{name}]");
            assert!(
                said[0].contains("has action \"window\" and no command"),
                "{said:?}"
            );
            let (_, said) = rules(&[section(name, &[("action", "\"windows\"")])]);
            let values = match name {
                PATH_RULE => "reveal, command, window, copy or off",
                _ => "open, command, window, copy or off",
            };
            assert!(said[0].contains(values), "{said:?}");
        }
        let (r, said) = rules(&[
            section(
                "tel",
                &[
                    ("action", "\"window\""),
                    ("command", "[\"w3m\", \"{url}\"]"),
                ],
            ),
            section(
                PATH_RULE,
                &[
                    ("action", "\"window\""),
                    ("command", "[\"hx\", \"{path}\"]"),
                ],
            ),
        ]);
        assert_eq!(
            said.len(),
            2,
            "neither program is on the fake PATH: {said:?}"
        );
        assert!(
            said.iter().all(|s| s.contains("is not on PATH")),
            "{said:?}"
        );
        let t = admit_with(b"tel:+1555", NONE, &r).expect("admitted");
        assert_eq!(t.action(), crate::link::Action::Window);
        assert_eq!(r.command_programs().collect::<Vec<_>>(), ["w3m"]);
        let path_var = Some(OsStr::new("/usr/bin:/bin"));
        assert_eq!(r.command_found("tel", path_var), Some(Program::Missing));
        assert_eq!(r.path_command_found(path_var), Some(Program::Missing));
        assert_eq!(
            r.told(),
            [
                "config: [link.tel] opens tel: links from program output in a new window running w3m",
                "config: [link.path] opens a clicked printed path in a new window running hx",
            ]
        );
        let (sh, said) = rules(&[section(
            "tel",
            &[
                ("action", "\"window\""),
                ("command", "[\"/bin/sh\", \"{url}\"]"),
            ],
        )]);
        assert_eq!(
            said,
            Vec::<String>::new(),
            "the control: found, nothing said"
        );
        assert_eq!(sh.command_found("tel", path_var), Some(Program::Found));
    }

    /// **W4: every value `ACTIONS` and `PATH_ACTIONS` list is one the reader
    /// takes, and the reader takes no other**: each read alone, or beside a
    /// command where it runs one, is a rule said nothing about, and a value
    /// spelled once more makes none. The guide and the example are held to
    /// the same lists (`config.rs`). Perturbation: drop `"window"` from
    /// `ACTIONS` -- the reader takes a value the list does not name.
    #[test]
    fn the_action_lists_are_the_readers() {
        let cmd = |name: &str| match name {
            PATH_RULE => "[\"/bin/sh\", \"{path}\"]",
            _ => "[\"/bin/sh\", \"{url}\"]",
        };
        for (name, values) in [("tel", &ACTIONS), (PATH_RULE, &PATH_ACTIONS)] {
            for v in [
                "open", "reveal", "command", "window", "copy", "off", "launch",
            ] {
                let quoted = format!("\"{v}\"");
                let alone = rules(&[section(name, &[("action", &quoted)])]);
                let with = rules(&[section(
                    name,
                    &[("action", &quoted), ("command", cmd(name))],
                )]);
                let reads = alone.1.is_empty() || with.1.is_empty();
                assert_eq!(reads, values.contains(&v), "[link.{name}] action {v}");
            }
        }
    }

    /// **W5: a window's program is looked for where the window starts**:
    /// one that is gone, by its full path or as a name on none of `PATH`'s
    /// folders, is missing, and both drivers open nothing for it; one that
    /// is there, by either spelling, is found. Perturbation: answer found in
    /// `Run::found` -- the missing editor opens a window that exits at once.
    #[test]
    fn a_windows_program_is_looked_for_where_it_starts() {
        let run = |p: &str| Run::given(vec![p.into(), "/tmp/x".into()]).expect("a program");
        let path_var = Some(OsStr::new("/usr/bin:/bin"));
        assert_eq!(run("/no/such/editor").found(path_var), Program::Missing);
        assert_eq!(run("no-such-editor").found(path_var), Program::Missing);
        assert_eq!(
            run("sh").found(None),
            Program::Missing,
            "no PATH to look on"
        );
        assert_eq!(
            run("/bin/sh").found(path_var),
            Program::Found,
            "the control"
        );
        assert_eq!(run("sh").found(path_var), Program::Found);
    }

    /// **DT-L2: a script flag with the placeholder attached to it is a
    /// script too**: `python3` given `-c{url}`, `perl -e{url}`, `bash -lc{url}`
    /// and `env -Ssh -c {url}` run the link's text as code, as the spelled-out
    /// forms `a_placeholder_in_a_script_is_refused_at_read` refuses do.
    /// Named break: take a short flag with a value attached (drop
    /// `Opt::Short`'s refusal in `optioned`).
    #[test]
    fn an_attached_script_flag_is_refused_at_read() {
        for cmd in [
            "[\"python3\", \"-c{url}\"]",
            "[\"perl\", \"-e{url}\"]",
            "[\"bash\", \"-lc{url}\"]",
            "[\"/usr/bin/env\", \"-Ssh -c {url}\"]",
        ] {
            let (r, said) = rules(&[section("tel", &[("command", cmd)])]);
            assert!(r.list().is_empty(), "{cmd}: read");
            assert_eq!(said.len(), 1, "{cmd}: {said:?}");
        }
    }

    /// **DT-L2: a placeholder in an option, for every program baud knows
    /// runs code and for one it does not**: for each entry of `PROGRAMS`,
    /// `env` among them, a value attached to any short flag -- each of a
    /// `Code::Flag` program's script flags too -- and in a `--name=` long
    /// option is refused at read, the warning saying to write the two
    /// apart; for a browser baud does not know, `--app={url}` is taken, as
    /// it has no form with the value apart, `-a{url}` is refused, and the
    /// two written apart are taken. Named breaks: take a long option for
    /// every program (drop `runs_code`'s arm) -- a shell's long option is
    /// read; refuse one for every program (ignore `runs_code`) -- the
    /// browser's `--app={url}` is refused.
    #[test]
    fn a_placeholder_in_an_option_is_refused_for_a_program_that_runs_code() {
        let refused = |cmd: &str| {
            let (r, said) = rules(&[section("tel", &[("command", cmd)])]);
            assert!(r.list().is_empty(), "{cmd}: read");
            assert_eq!(said.len(), 1, "{cmd}: {said:?}");
            said.into_iter().next().unwrap_or_default()
        };
        for (name, code) in PROGRAMS {
            let _ = refused(&format!("[\"{name}\", \"--name={{url}}\"]"));
            let _ = refused(&format!("[\"{name}\", \"-z{{url}}\"]"));
            let _ = refused(&format!(
                "[\"/usr/bin/env\", \"{name}\", \"--name={{url}}\"]"
            ));
            if let Code::Flag(flags) = code {
                for f in flags {
                    let _ = refused(&format!("[\"{name}\", \"-{f}{{url}}\"]"));
                }
            }
        }
        let long = refused("[\"sh\", \"--name={url}\"]");
        assert!(
            long.contains("two elements") && long.contains("script of your own"),
            "{long}"
        );
        assert!(
            read_cmd("[\"/usr/bin/chromium\", \"--app={url}\"]"),
            "--app="
        );
        assert!(
            read_cmd("[\"/usr/bin/chromium\", \"--app\", \"{url}\"]"),
            "apart"
        );
        let short = refused("[\"/usr/bin/chromium\", \"-a{url}\"]");
        assert!(short.contains("two elements"), "{short}");
    }

    /// **An argument is read once into its pieces**: text alone is text;
    /// `{path}:{line}:{col}` is three values with the text between; and a
    /// filled value that spells a placeholder is put in as it is, never read
    /// as one; and a rule's arguments read back as written.
    #[test]
    fn an_argument_is_read_once_into_its_pieces() {
        let (r, said) = rules(&[section(
            "tel",
            &[("command", "[\"/usr/bin/dial\", \"-o\", \"a{url}b\"]")],
        )]);
        match r.rule_for("tel").map(|(_, x)| x.act()) {
            Some(Act::Command(argv)) => assert_eq!(argv.written(), ["-o", "a{url}b"]),
            other => panic!("{other:?}: {said:?}"),
        }
        assert_eq!(Element::read("--goto"), Element::Literal("--goto".into()));
        assert_eq!(
            Element::read("{path}:{line}:{col}"),
            Element::Pieces(vec![
                Piece::Value(Placeholder::Path),
                Piece::Lit(":".into()),
                Piece::Value(Placeholder::Line),
                Piece::Lit(":".into()),
                Piece::Value(Placeholder::Col),
            ])
        );
        let filled = Element::read("x{url}y").filled(&[(Placeholder::Url, b"{url}", Value::Uri)]);
        assert_eq!(filled, Ok((b"x{url}y".to_vec(), Put::Yes)));
    }
}
