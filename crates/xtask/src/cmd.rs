//! A command a gate runs: its variables and its words, run from the
//! repository's root with this process's output.
//!
//! `cargo` is the cargo that runs this program (`CARGO`), and `cargo xtask`
//! is this program itself, so a gate that runs another gate builds nothing
//! more.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::third_party::Outcome;

/// A command: the variables it sets and its words, the program first.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Cmd {
    /// Variables set for it alone, in order.
    pub env: Vec<(String, String)>,
    /// The program and its arguments.
    pub argv: Vec<String>,
}

impl Cmd {
    /// The command `words` spell.
    pub fn new(words: &[&str]) -> Cmd {
        Cmd {
            env: Vec::new(),
            argv: words.iter().map(|w| w.to_string()).collect(),
        }
    }

    /// The command with `name` set to `value`.
    pub fn var(mut self, name: &str, value: &str) -> Cmd {
        self.env.push((name.to_string(), value.to_string()));
        self
    }

    /// The command with one more argument.
    pub fn arg(mut self, a: impl Into<String>) -> Cmd {
        self.argv.push(a.into());
        self
    }

    /// The process this command starts, from `root`: `cargo xtask` is this
    /// program and `cargo` is `CARGO` when it is set. `None` for a command
    /// with no words.
    pub fn command(&self, root: &Path) -> Option<Command> {
        let cargo = || env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
        let (program, rest): (OsString, &[String]) = match self.argv.as_slice() {
            [] => return None,
            [c, x, rest @ ..] if c == "cargo" && x == "xtask" => match env::current_exe() {
                Ok(me) => (me.into(), rest),
                Err(_) => (cargo(), &self.argv[1..]),
            },
            [c, rest @ ..] if c == "cargo" => (cargo(), rest),
            [p, rest @ ..] => (p.into(), rest),
        };
        let mut c = Command::new(program);
        c.args(rest).current_dir(root).stdin(Stdio::null());
        for (k, v) in &self.env {
            c.env(k, v);
        }
        Some(c)
    }

    /// Runs it from `root` with this process's output: whether it exited 0.
    /// One that cannot start is named on stderr and fails.
    pub fn status(&self, root: &Path) -> Outcome {
        let Some(mut c) = self.command(root) else {
            return Outcome::Failed;
        };
        match c.status() {
            Ok(s) if s.success() => Outcome::Passed,
            Ok(_) => Outcome::Failed,
            Err(e) => {
                eprintln!("{}: {e}", self.argv[0]);
                Outcome::Failed
            }
        }
    }
}

/// The command as a shell would spell it: each variable as `NAME=value`,
/// then the words, separated by spaces.
impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let words: Vec<String> = self
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .chain(self.argv.iter().cloned())
            .collect();
        f.write_str(&words.join(" "))
    }
}
