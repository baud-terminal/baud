//! Command lines read by the rules of Python 3.13's `argparse`, so a line
//! the scripts took is taken and one they refused is refused with the same
//! exit code: long options by any unambiguous prefix, `--x=v`, `-h` anywhere
//! before an error, a negative number or anything with a space as a value,
//! exit 2 on a refusal, and every word from the first positional on, options
//! included, handed to the command (`nargs=REMAINDER`). The order is theirs
//! too: values are converted as they are reached, so the first bad one is
//! the one named.

use std::ffi::OsString;

use super::py;

/// How a value is checked when it is reached.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conv {
    /// Any text.
    Text,
    /// `float(value)`.
    Float,
    /// `int(value)`.
    Int,
}

/// What an option takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Takes {
    /// One value; the last given wins.
    One(Conv),
    /// One value each time, all kept.
    Append,
    /// One or more values, the last occurrence's.
    Many,
}

/// Whether leaving an option out is a refusal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Need {
    /// `required=True`.
    Required,
    /// The default.
    Optional,
}

/// One option.
#[derive(Clone, Copy, Debug)]
pub struct Opt {
    /// Its one long spelling.
    pub flag: &'static str,
    /// What it takes.
    pub takes: Takes,
    /// Whether it must be given.
    pub need: Need,
}

/// Whether the parser ends in a `nargs=REMAINDER` positional.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rest {
    /// It does: `cmd`.
    Command,
    /// It takes no positional, and one is an unrecognized argument.
    Nothing,
}

/// A parser: its options in the order they were added.
pub struct Parser {
    /// `prog` in its error lines.
    pub prog: &'static str,
    /// Its `format_usage()`.
    pub usage: &'static str,
    /// Its `format_help()` past the usage.
    pub help: &'static str,
    /// Its options, past `-h`.
    pub opts: &'static [Opt],
    /// Whether it ends in a command.
    pub rest: Rest,
}

/// What a command line said.
#[derive(Debug, Default)]
pub struct Parsed {
    values: Vec<(&'static str, OsString)>,
    /// The command, every word after the options, `--` included.
    pub cmd: Vec<OsString>,
}

impl Parsed {
    /// The last value of `flag`.
    pub fn one(&self, flag: &str) -> Option<&OsString> {
        self.values
            .iter()
            .rev()
            .find(|(f, _)| *f == flag)
            .map(|(_, v)| v)
    }

    /// Every value of `flag` kept, in order.
    pub fn all(&self, flag: &str) -> Vec<&OsString> {
        self.values
            .iter()
            .filter(|(f, _)| *f == flag)
            .map(|(_, v)| v)
            .collect()
    }

    /// `flag`'s value as text, or `default`.
    pub fn text(&self, flag: &str, default: &str) -> String {
        self.one(flag)
            .map_or_else(|| default.to_string(), |v| v.to_string_lossy().into_owned())
    }

    /// `flag`'s value as a float, already checked, or `default`.
    pub fn float(&self, flag: &str, default: f64) -> f64 {
        self.one(flag)
            .and_then(|v| py::float(&v.to_string_lossy()))
            .unwrap_or(default)
    }

    /// `flag`'s value as an integer, already checked, or `default`.
    pub fn int(&self, flag: &str, default: i64) -> i64 {
        self.one(flag)
            .and_then(|v| py::int(&v.to_string_lossy()))
            .unwrap_or(default)
    }

    /// The command's words, every `--` taken out: the scripts did, so one
    /// meant for the program never reached it either.
    pub fn command(&self) -> Vec<OsString> {
        self.cmd.iter().filter(|c| *c != "--").cloned().collect()
    }
}

/// Which option one spelling names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Help,
    Option(usize),
}

/// One reading of an option word.
#[derive(Clone, Debug)]
struct Reading {
    /// `None` when no option has that name.
    action: Option<Action>,
    spelling: String,
    sep: String,
    explicit: Option<String>,
}

/// How a parse stopped early: the exit code, the text already printed.
pub type Stop = i32;

impl Parser {
    /// Every spelling, in the order `argparse` searches them.
    fn spellings(&self) -> Vec<(String, Action)> {
        let mut all = vec![
            ("-h".to_string(), Action::Help),
            ("--help".to_string(), Action::Help),
        ];
        for (i, o) in self.opts.iter().enumerate() {
            all.push((o.flag.to_string(), Action::Option(i)));
        }
        all
    }

    /// `_parse_optional`: `None` when `word` is a positional.
    fn readings(&self, word: &str) -> Option<Vec<Reading>> {
        if !word.starts_with('-') {
            return None;
        }
        let names = self.spellings();
        let exact = |w: &str| names.iter().find(|(n, _)| n == w).map(|(_, a)| *a);
        if let Some(a) = exact(word) {
            return Some(vec![Reading {
                action: Some(a),
                spelling: word.to_string(),
                sep: String::new(),
                explicit: None,
            }]);
        }
        if word.len() == 1 {
            return None;
        }
        if let Some((name, value)) = word.split_once('=') {
            if let Some(a) = exact(name) {
                return Some(vec![Reading {
                    action: Some(a),
                    spelling: name.to_string(),
                    sep: "=".to_string(),
                    explicit: Some(value.to_string()),
                }]);
            }
        }
        let found = self.prefixed(word, &names);
        if !found.is_empty() {
            return Some(found);
        }
        if negative_number(word) || word.contains(' ') {
            return None;
        }
        Some(vec![Reading {
            action: None,
            spelling: word.to_string(),
            sep: String::new(),
            explicit: None,
        }])
    }

    /// What a word abbreviates: argparse's search of the option tuples.
    fn prefixed(&self, word: &str, names: &[(String, Action)]) -> Vec<Reading> {
        let (prefix, sep, explicit) = match word.split_once('=') {
            Some((p, e)) => (p, "=", Some(e.to_string())),
            None => (word, "", None),
        };
        let mut found = Vec::new();
        if word.starts_with("--") {
            for (name, a) in names {
                if name.starts_with(prefix) {
                    found.push(Reading {
                        action: Some(*a),
                        spelling: name.clone(),
                        sep: sep.to_string(),
                        explicit: explicit.clone(),
                    });
                }
            }
        } else {
            let mut cut = word.char_indices().map(|(i, _)| i);
            cut.next();
            cut.next();
            let tail = cut.next().unwrap_or(word.len());
            let (short, short_tail) = word.split_at(tail);
            for (name, a) in names {
                if name == short {
                    found.push(Reading {
                        action: Some(*a),
                        spelling: name.clone(),
                        sep: String::new(),
                        explicit: Some(short_tail.to_string()),
                    });
                } else if name.starts_with(prefix) {
                    found.push(Reading {
                        action: Some(*a),
                        spelling: name.clone(),
                        sep: sep.to_string(),
                        explicit: explicit.clone(),
                    });
                }
            }
        }
        found
    }

    /// A refusal: usage and the message on stderr, exit 2.
    fn refuse(&self, msg: &str) -> Stop {
        eprint!("{}", self.usage);
        eprintln!("{}: error: {msg}", self.prog);
        2
    }

    /// The action's name in an error.
    fn named(&self, a: Action) -> String {
        match a {
            Action::Help => "-h/--help".to_string(),
            Action::Option(i) => self.opts[i].flag.to_string(),
        }
    }

    /// `parse_args`. `Err` is the exit code, the text printed.
    pub fn parse(&self, args: &[OsString]) -> Result<Parsed, Stop> {
        let words: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        // The pattern: `A` a value, `O` an option word, `-` the first `--`,
        // past which every word is a value.
        let mut pattern = Vec::with_capacity(words.len());
        let mut options: Vec<Option<Vec<Reading>>> = Vec::with_capacity(words.len());
        let mut after = false;
        for w in &words {
            if after {
                pattern.push(b'A');
                options.push(None);
            } else if w == "--" {
                pattern.push(b'-');
                options.push(None);
                after = true;
            } else {
                let r = self.readings(w);
                pattern.push(if r.is_some() { b'O' } else { b'A' });
                options.push(r);
            }
        }
        let mut walk = Walk {
            parser: self,
            args,
            words: &words,
            pattern: &pattern,
            options: &options,
            parsed: Parsed::default(),
            extras: Vec::new(),
            command: match self.rest {
                Rest::Command => Command::Waiting,
                Rest::Nothing => Command::Done,
            },
        };
        let last = options.iter().rposition(Option::is_some);
        let mut start = 0;
        while last.is_some_and(|l| start <= l) {
            let next = (start..words.len())
                .find(|i| options[*i].is_some())
                .unwrap_or(words.len());
            if start != next {
                let end = walk.positionals(Word(start)).0;
                if end > start {
                    start = end;
                    continue;
                }
                walk.extras.extend_from_slice(&words[start..next]);
                start = next;
            }
            start = walk.optional(Word(start))?.0;
        }
        let stop = walk.positionals(Word(start)).0;
        walk.extras.extend_from_slice(&words[stop..]);
        let Walk { parsed, extras, .. } = walk;
        let missing: Vec<&str> = self
            .opts
            .iter()
            .filter(|o| o.need == Need::Required && parsed.one(o.flag).is_none())
            .map(|o| o.flag)
            .collect();
        if !missing.is_empty() {
            return Err(self.refuse(&format!(
                "the following arguments are required: {}",
                missing.join(", ")
            )));
        }
        if !extras.is_empty() {
            return Err(self.refuse(&format!("unrecognized arguments: {}", extras.join(" "))));
        }
        Ok(parsed)
    }
}

/// `^-\d+$|^-\d*\.\d+$`.
fn negative_number(word: &str) -> bool {
    let Some(tail) = word.strip_prefix('-') else {
        return false;
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    match tail.split_once('.') {
        None => digits(tail),
        Some((whole, frac)) => (whole.is_empty() || digits(whole)) && digits(frac),
    }
}

/// An index into the command line's words.
#[derive(Clone, Copy)]
struct Word(usize);

/// `consume_positionals` and `consume_optional`, over one command line.
struct Walk<'a> {
    parser: &'a Parser,
    args: &'a [OsString],
    words: &'a [String],
    pattern: &'a [u8],
    options: &'a [Option<Vec<Reading>>],
    parsed: Parsed,
    extras: Vec<String>,
    command: Command,
}

/// Where the command's positional stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    /// It has yet to take its words.
    Waiting,
    /// It has them, or the parser has no such positional.
    Done,
}

impl Walk<'_> {
    /// `consume_positionals`: the command takes everything left, once.
    fn positionals(&mut self, start: Word) -> Word {
        if self.command == Command::Waiting {
            self.command = Command::Done;
            self.parsed.cmd = self.args[start.0..].to_vec();
            Word(self.args.len())
        } else {
            start
        }
    }

    /// One value through its conversion.
    fn take(&mut self, opt: Opt, value: OsString) -> Result<(), Stop> {
        if let Takes::One(conv) = opt.takes {
            let text = value.to_string_lossy();
            let refused = match conv {
                Conv::Text => None,
                Conv::Float => py::float(&text).is_none().then_some("float"),
                Conv::Int => py::int(&text).is_none().then_some("int"),
            };
            if let Some(kind) = refused {
                let shown = py::str_repr(&text);
                return Err(self.parser.refuse(&format!(
                    "argument {}: invalid {kind} value: {shown}",
                    opt.flag
                )));
            }
        }
        self.parsed.values.push((opt.flag, value));
        Ok(())
    }

    /// `consume_optional` from the option word at `start`.
    fn optional(&mut self, start: Word) -> Result<Word, Stop> {
        let start = start.0;
        let readings = self.options[start].as_ref().expect("an option word");
        if readings.len() > 1 {
            let names: Vec<&str> = readings.iter().map(|r| r.spelling.as_str()).collect();
            return Err(self.parser.refuse(&format!(
                "ambiguous option: {} could match {}",
                self.words[start],
                names.join(", ")
            )));
        }
        let Reading {
            action,
            mut spelling,
            mut sep,
            mut explicit,
        } = readings[0].clone();
        let Some(mut action) = action else {
            self.extras.push(self.words[start].clone());
            return Ok(Word(start + 1));
        };
        // The actions this word names, taken only once it is all read.
        let mut helps = false;
        let mut one: Vec<(usize, Vec<OsString>)> = Vec::new();
        let stop;
        loop {
            let name = self.parser.named(action);
            let Some(e) = explicit.clone() else {
                let from = start + 1;
                match action {
                    Action::Help => {
                        helps = true;
                        stop = from;
                    }
                    Action::Option(i) => {
                        let run = self.pattern[from.min(self.pattern.len())..]
                            .iter()
                            .take_while(|p| **p == b'A')
                            .count();
                        let takes = self.parser.opts[i].takes;
                        let count = if takes == Takes::Many {
                            run
                        } else {
                            run.min(1)
                        };
                        if count == 0 {
                            let wanted = if takes == Takes::Many {
                                "expected at least one argument"
                            } else {
                                "expected one argument"
                            };
                            return Err(self.parser.refuse(&format!("argument {name}: {wanted}")));
                        }
                        stop = from + count;
                        one.push((i, self.args[from..stop].to_vec()));
                    }
                }
                break;
            };
            match action {
                Action::Option(i) => {
                    one.push((i, vec![OsString::from(e)]));
                    stop = start + 1;
                    break;
                }
                Action::Help => {
                    let single = !spelling.starts_with("--");
                    if !single || !sep.is_empty() || e.starts_with('-') {
                        let shown = py::str_repr(&e);
                        return Err(self.parser.refuse(&format!(
                            "argument {name}: ignored explicit argument {shown}"
                        )));
                    }
                    helps = true;
                    let mut chars = e.chars();
                    let c = chars.next().expect("a tail that is not empty");
                    spelling = format!("-{c}");
                    if spelling == "-h" {
                        action = Action::Help;
                        let tail = chars.as_str();
                        if tail.is_empty() {
                            sep = String::new();
                            explicit = None;
                        } else if let Some(v) = tail.strip_prefix('=') {
                            sep = "=".to_string();
                            explicit = Some(v.to_string());
                        } else {
                            sep = String::new();
                            explicit = Some(tail.to_string());
                        }
                    } else {
                        self.extras.push(format!("-{e}"));
                        stop = start + 1;
                        break;
                    }
                }
            }
        }
        if helps {
            print!("{}{}", self.parser.usage, self.parser.help);
            return Err(0);
        }
        for (i, values) in one {
            // `nargs="+"` stores: a later occurrence replaces the earlier.
            if self.parser.opts[i].takes == Takes::Many {
                let flag = self.parser.opts[i].flag;
                self.parsed.values.retain(|(f, _)| *f != flag);
            }
            for v in values {
                self.take(self.parser.opts[i], v)?;
            }
        }
        Ok(Word(stop))
    }
}
