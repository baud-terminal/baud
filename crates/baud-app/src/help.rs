//! **`baud --help`**: a usage summary naming every argument baud reads,
//! printed to stdout before anything is set up, as `--version` and
//! `--credits` are: no window, no config read or written, no shell, no
//! network, and exit 0.

use std::ffi::OsString;
use std::io::{self, Write};

use crate::run_arg::{FLAG, GRID};

/// The flag.
pub(crate) const HELP: &str = "--help";

/// **`baud --help`**: only the first argument, and only that spelling, as
/// `--version` is asked. After `-e` every argument is the program's, so
/// `baud -e man --help` runs `man --help`.
pub(crate) fn asks<I: IntoIterator<Item = OsString>>(args: I) -> bool {
    args.into_iter().next().is_some_and(|first| first == HELP)
}

/// **The usage summary**: each form a start takes, then each flag and
/// what it does.
pub(crate) fn text() -> String {
    let flag = |name: &str, what: &str| format!("  {name:<22}{what}\n");
    let run = format!("{FLAG} PROGRAM [ARGS...]");
    let grid = format!("{GRID} COLSxROWS");
    [
        "Usage: baud\n".to_owned(),
        format!("  or:  baud [{grid}] {run}\n"),
        format!("  or:  baud {HELP} | --version | --credits\n"),
        "Open a terminal window running your shell.\n\n".to_owned(),
        flag(&run, "run PROGRAM with ARGS in place of the shell; every"),
        flag(
            "",
            "argument after PROGRAM is passed to it as it is (Linux)",
        ),
        flag(
            &grid,
            &format!("open COLS columns by ROWS rows; before {FLAG}"),
        ),
        flag(HELP, "print this summary and exit"),
        flag(
            "--version",
            "print the version, copyright and licence and exit",
        ),
        flag("--credits", "print the third-party notices and exit"),
        format!("\n{HELP}, --version and --credits are read only as the first argument.\n"),
    ]
    .concat()
}

/// **The summary onto `out`**: a reader that stops early, as
/// `baud --help | head` does, is no failure.
pub(crate) fn print(out: &mut impl Write) -> io::Result<()> {
    match out.write_all(text().as_bytes()).and_then(|()| out.flush()) {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        done => done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(args: &[&str]) -> bool {
        asks(args.iter().map(OsString::from))
    }

    /// **Only `--help` as the first argument prints and leaves**, as
    /// `--version` is asked; `-e`'s program keeps a `--help` of its own.
    /// Perturbation: `asks` answering for any argument fails the second
    /// half.
    #[test]
    fn only_help_first_asks() {
        assert!(asked(&["--help"]));
        assert!(asked(&["--help", "extra"]));
        assert!(!asked(&[]));
        assert!(!asked(&["-h"]));
        assert!(!asked(&["--helps"]));
        assert!(!asked(&["extra", "--help"]));
        assert!(!asked(&["-e", "man", "--help"]));
        assert!(!asked(&["--grid", "80x24", "-e", "man", "--help"]));
        assert!(!crate::version::asks([OsString::from("--help")]));
        assert!(!crate::credits::asks([OsString::from("--help")]));
    }

    /// **The summary names every argument `main` reads**: `-e` and
    /// `--grid` from `run_arg`'s own constants, and the three that print
    /// and exit. Perturbation: a flag dropped from [`text`] fails its line.
    #[test]
    fn the_summary_names_every_flag() {
        let summary = text();
        for flag in [FLAG, GRID, HELP, "--version", "--credits"] {
            assert!(summary.contains(flag), "{flag} in:\n{summary}");
        }
        assert!(summary.contains("-e PROGRAM [ARGS...]"), "{summary}");
        assert!(summary.contains("--grid COLSxROWS"), "{summary}");
        assert!(summary.starts_with("Usage: baud\n"), "{summary}");
        assert!(summary.ends_with('\n'));
        assert!(
            summary.lines().all(|l| l.chars().count() <= 80),
            "every line fits 80 columns:\n{summary}"
        );
    }

    /// **The bytes printed are [`text`]'s, and a closed reader is no
    /// failure.**
    #[test]
    fn it_prints_the_summary_and_a_closed_pipe_is_fine() {
        let mut out = Vec::new();
        print(&mut out).expect("into memory");
        assert_eq!(out, text().as_bytes());

        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        print(&mut Closed).expect("a closed pipe ends the print quietly");
    }

    /// **`main` answers before anything is set up**, as it answers
    /// `--version`: before the startup clock, the log, the font thread,
    /// `-e`'s reading and either driver, so `--help` opens no window and
    /// reads no config. In the source, since running it to find out is the
    /// failure itself. Perturbation: move the check below `app::run()` and
    /// this fails.
    #[test]
    fn main_answers_before_it_starts_anything() {
        const SRC: &str = include_str!("main.rs");
        let body = &SRC[SRC.find(concat!("fn ", "main() {")).expect("main")..];
        let asked = body.find(concat!("help::", "asks(")).expect("main asks");
        for later in [
            concat!("run_arg::", "asked("),
            concat!("host::", "startup_begin()"),
            concat!("log::", "init()"),
            concat!("discover::", "Catalogue"),
            concat!("app::", "run()"),
            concat!("linux::", "run("),
        ] {
            assert!(
                asked < body.find(later).expect(later),
                "the answer comes before {later}"
            );
        }
    }
}
