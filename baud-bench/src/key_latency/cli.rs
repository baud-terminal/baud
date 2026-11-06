//! The command line, by argparse's rules: an option may be shortened to any
//! prefix only it has, a value may follow `=` or the next argument, and
//! usage errors exit 2.

use super::py;
use super::stats::Ms;

/// The help text, as argparse lays it out at 80 columns.
pub const HELP: &str = "\
usage: key-latency [-h] [--selftest] [--rounds ROUNDS] [--keys KEYS]
                   [--gap GAP] [--child {cat,shell,none,stream,mute}]
                   [--arm ARM] [--aa] [--grid GRID] [--chars CHARS]
                   [--scratch SCRATCH]

keystroke-to-pixel latency for baud

options:
  -h, --help            show this help message and exit
  --selftest            run the parser/pairing/statistics unit tests and exit
  --rounds ROUNDS
  --keys KEYS
  --gap GAP             MEAN ms between keys; the helper jitters each gap
                        uniformly by +-25% (0 stays 0: typeahead, the guard
                        bounds the rate)
  --child {cat,shell,none,stream,mute}
                        cat: the tty's own echo; shell: the login shell; none:
                        `stty -echo; sleep 60`, the no-echo control; mute: a
                        prompt (OSC 133 A and B) that never echoes, the row-9
                        expiry-and-hold control; stream: a line printed every
                        2 ms under the tty's echo, the row-6 workload
  --arm ARM             name=path/to/baud (repeatable)
  --aa                  add an A/A arm running the first arm's binary again
  --grid GRID
  --chars CHARS         plain letters the helper cycles through
  --scratch SCRATCH
";

/// The child under the app's pty.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Child {
    /// The tty's own echo.
    Cat,
    /// The login shell.
    Shell,
    /// `stty -echo; sleep 60`, the no-echo control.
    None,
    /// A line printed every 2 ms under the tty's echo.
    Stream,
    /// A prompt that never echoes.
    Mute,
}

impl Child {
    /// Its name on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Child::Cat => "cat",
            Child::Shell => "shell",
            Child::None => "none",
            Child::Stream => "stream",
            Child::Mute => "mute",
        }
    }

    fn of(s: &str) -> Option<Child> {
        [
            Child::Cat,
            Child::Shell,
            Child::None,
            Child::Stream,
            Child::Mute,
        ]
        .into_iter()
        .find(|c| c.name() == s)
    }
}

/// Whether a sweep runs an A/A arm.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AaArm {
    /// The first arm's binary runs again as `aa`.
    Run,
    /// It does not.
    Skip,
}

/// A sweep's arguments.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Args {
    /// Rounds, the first discarded when there are more.
    pub rounds: i128,
    /// Keys a run.
    pub keys: i128,
    /// The mean inter-key gap.
    pub gap: Ms,
    /// The child under the pty.
    pub child: Child,
    /// `name=path`, repeatable.
    pub arm: Vec<String>,
    /// The A/A arm.
    pub aa: AaArm,
    /// `BAUD_GRID`.
    pub grid: String,
    /// The letters the helper cycles through.
    pub chars: String,
    /// Where the raw rows go. `None` is `$BAUD_SCRATCH` or the temp directory.
    pub scratch: Option<String>,
}

/// What the command line asks for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Cli {
    /// `--help`.
    Help,
    /// `--selftest`.
    Selftest,
    /// A sweep.
    Sweep(Args),
    /// A usage error, for stderr; the exit is 2.
    Usage(String),
}

const VALUED: [&str; 8] = [
    "--rounds",
    "--keys",
    "--gap",
    "--child",
    "--arm",
    "--grid",
    "--chars",
    "--scratch",
];
const FLAGS: [&str; 4] = ["-h", "--help", "--selftest", "--aa"];

/// `^-\d+$|^-\d*\.\d+$`.
fn negative_number(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('-') else {
        return false;
    };
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    match rest.split_once('.') {
        None => !rest.is_empty() && digits(rest),
        Some((whole, frac)) => digits(whole) && !frac.is_empty() && digits(frac),
    }
}

/// One argument as argparse sorts it.
enum Kind<'a> {
    /// Not an option.
    Positional,
    /// A known option, and the value it was given with `=` or glued on.
    Option(&'static str, Option<&'a str>),
    /// Looks like an option and is none.
    Unknown,
    /// A prefix more than one option has.
    Ambiguous,
}

fn classify(arg: &str) -> Kind<'_> {
    if !arg.starts_with('-') {
        return Kind::Positional;
    }
    let every = || VALUED.iter().chain(FLAGS.iter()).copied();
    if let Some(o) = every().find(|o| *o == arg) {
        return Kind::Option(o, None);
    }
    if arg.len() == 1 {
        return Kind::Positional;
    }
    if let Some((name, value)) = arg.split_once('=') {
        if let Some(o) = every().find(|o| *o == name) {
            return Kind::Option(o, Some(value));
        }
    }
    let matches: Vec<(&'static str, Option<&str>)> = if arg.starts_with("--") {
        let (prefix, value) = match arg.split_once('=') {
            Some((p, v)) => (p, Some(v)),
            None => (arg, None),
        };
        every()
            .filter(|o| o.starts_with(prefix))
            .map(|o| (o, value))
            .collect()
    } else {
        // `-hx`: the short option with the rest glued on.
        every()
            .filter(|o| *o == arg.get(..2).unwrap_or(arg))
            .map(|o| (o, arg.get(2..)))
            .collect()
    };
    match matches.as_slice() {
        [(o, v)] => return Kind::Option(o, *v),
        [] => {}
        _ => return Kind::Ambiguous,
    }
    if negative_number(arg) || arg.contains(' ') {
        return Kind::Positional;
    }
    Kind::Unknown
}

/// Read the arguments, left to right as argparse does: `--help` answers when
/// it is reached, and what nobody recognised is an error at the end.
pub fn parse(argv: &[String]) -> Cli {
    let mut args = Args {
        rounds: 5,
        keys: 30,
        gap: Ms(150),
        child: Child::Cat,
        arm: Vec::new(),
        aa: AaArm::Skip,
        grid: "120x40".to_string(),
        chars: "asdfjkl".to_string(),
        scratch: None,
    };
    let mut selftest = false;
    let mut extras: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        i += 1;
        if arg == "--" {
            extras.extend(argv[i - 1..].iter().map(String::as_str));
            break;
        }
        let (option, glued) = match classify(arg) {
            Kind::Positional | Kind::Unknown => {
                extras.push(arg);
                continue;
            }
            Kind::Ambiguous => {
                return Cli::Usage(format!("ambiguous option: {arg}"));
            }
            Kind::Option(o, v) => (o, v),
        };
        if FLAGS.contains(&option) {
            // `-hx` still runs `-h`; `-h=x`, `-h-x` and `--help=x` are errors.
            if glued.is_some() && option != "-h" {
                return Cli::Usage(format!("{option}: ignored explicit argument"));
            }
            if option == "-h" && glued.is_some() {
                let tail = arg[2..].trim_start_matches('h');
                if tail.starts_with(['=', '-']) {
                    let v = tail.strip_prefix('=').unwrap_or(tail);
                    return Cli::Usage(format!(
                        "argument -h/--help: ignored explicit argument {}",
                        py::repr_str(v)
                    ));
                }
            }
            match option {
                "-h" | "--help" => return Cli::Help,
                "--selftest" => selftest = true,
                _ => args.aa = AaArm::Run,
            }
            continue;
        }
        let value = match glued {
            Some(v) => v.to_string(),
            None => {
                let usable = argv
                    .get(i)
                    .filter(|v| *v != "--" && matches!(classify(v), Kind::Positional));
                let Some(v) = usable else {
                    return Cli::Usage(format!("argument {option}: expected one argument"));
                };
                i += 1;
                v.clone()
            }
        };
        let int = |v: &str| py::int(v);
        let bad_int = || Cli::Usage(format!("argument {option}: invalid int value: {value:?}"));
        match option {
            "--rounds" => match int(&value) {
                Some(n) => args.rounds = n,
                None => return bad_int(),
            },
            "--keys" => match int(&value) {
                Some(n) => args.keys = n,
                None => return bad_int(),
            },
            "--gap" => match int(&value) {
                Some(n) => args.gap = Ms(n),
                None => return bad_int(),
            },
            "--child" => match Child::of(&value) {
                Some(c) => args.child = c,
                None => {
                    return Cli::Usage(format!(
                        "argument --child: invalid choice: {}",
                        py::repr_str(&value)
                    ));
                }
            },
            "--arm" => args.arm.push(value),
            "--grid" => args.grid = value,
            "--chars" => args.chars = value,
            _ => args.scratch = Some(value),
        }
    }
    if !extras.is_empty() {
        return Cli::Usage(format!("unrecognized arguments: {}", extras.join(" ")));
    }
    if selftest {
        return Cli::Selftest;
    }
    Cli::Sweep(args)
}
