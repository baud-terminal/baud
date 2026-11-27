//! **`baud --credits`**: the notices of every third-party work this build
//! carries, offline; the About window's acknowledgements line opens the
//! same notices in THIRD_PARTY.md, which carries them too.
//!
//! The text is `assets/CREDITS.txt`, which `cargo xtask credits` writes
//! from `docs/research/third-party.toml` and the licence texts its rows
//! point to, and whose `--check` fails when an include the binary embeds
//! falls under no row. It is printed as it is, byte for byte, and like
//! `--version` before anything is set up: no window, no config read or
//! written, no shell, no network.

use std::ffi::OsString;
use std::io::{self, Write};

/// The notices, as the build embedded them.
pub(crate) const TEXT: &str = include_str!("../assets/CREDITS.txt");

/// **`baud --credits`**: only the first argument, and only that spelling,
/// as `--version` is asked.
pub(crate) fn asks<I: IntoIterator<Item = OsString>>(args: I) -> bool {
    args.into_iter()
        .next()
        .is_some_and(|first| first == "--credits")
}

/// **The notices onto `out`, exactly**: a reader that stops early, as
/// `baud --credits | head` does, is no failure.
pub(crate) fn print(out: &mut impl Write) -> io::Result<()> {
    match out.write_all(TEXT.as_bytes()).and_then(|()| out.flush()) {
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

    /// **Only `--credits` as the first argument prints and leaves**, as
    /// `--version` is asked. Perturbation: `asks` answering for any
    /// argument fails the second half.
    #[test]
    fn only_credits_first_asks() {
        assert!(asked(&["--credits"]));
        assert!(asked(&["--credits", "extra"]));
        assert!(!asked(&[]));
        assert!(!asked(&["--credit"]));
        assert!(!asked(&["--version"]));
        assert!(!asked(&["extra", "--credits"]));
    }

    /// **The bytes printed are the file's, and a closed reader is no
    /// failure.** Perturbation: a `println!` adds a newline the file does
    /// not end in twice, and the first assertion fails.
    #[test]
    fn it_prints_the_file_exactly_and_a_closed_pipe_is_fine() {
        let mut out = Vec::new();
        print(&mut out).expect("into memory");
        assert_eq!(out, TEXT.as_bytes());
        assert!(TEXT.starts_with("Third-party notices for what this build of baud includes.\n"));

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
    /// `--version`: before the startup clock, the log, the font thread and
    /// either driver, so `--credits` opens no window and reads no config.
    /// In the source, since running it to find out is the failure itself;
    /// `tests/credits.rs` runs the binary only once this holds.
    /// Perturbation: move the check below `log::init()` and this fails.
    #[test]
    fn main_answers_before_it_starts_anything() {
        const SRC: &str = include_str!("main.rs");
        let body = &SRC[SRC.find(concat!("fn ", "main() {")).expect("main")..];
        let asked = body.find(concat!("credits::", "asks(")).expect("main asks");
        for later in [
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
