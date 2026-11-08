//! **`baud -e PROGRAM [ARGS...]`: a window running PROGRAM in place of a
//! shell**, the spelling `kitty -e`, `alacritty -e` and `wezterm start --`
//! give it. It is how a link's `action = "window"` opens a window on Linux,
//! whose driver runs one window per process: the click starts this binary
//! again with `-e` and the rule's argv after it (`linux.rs`). Pure over the
//! arguments, so its tests run on every platform.
//!
//! **Everything after `-e` is the argv, never re-split**: each argument is
//! one element as the caller's `execve` passed it, a space, a quote or a
//! `--` inside it included, and none is read as an option of baud's. `-e`
//! is read only as the first argument, as `--version` is, so nothing before
//! it is skipped unread.
#![forbid(unsafe_code)]

use std::ffi::OsString;

/// The flag.
pub(crate) const FLAG: &str = "-e";

/// What the arguments ask.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Asked {
    /// No `-e`: a shell window, as a launch with no arguments.
    Shell,
    /// `-e` and the argv after it.
    Run(crate::rules::Run),
    /// `-e` with no program after it, which is said and refused.
    NoProgram,
}

/// **What `args`, the arguments after the program's own name, ask.**
pub(crate) fn asked<I: IntoIterator<Item = OsString>>(args: I) -> Asked {
    let mut args = args.into_iter();
    match args.next() {
        Some(first) if first == FLAG => match crate::rules::Run::given(args.collect()) {
            Some(run) => Asked::Run(run),
            None => Asked::NoProgram,
        },
        _ => Asked::Shell,
    }
}

/// **The arguments that start a window running `run`**: [`FLAG`], then the
/// argv's elements, each one argument, moved.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) fn args(run: crate::rules::Run) -> impl Iterator<Item = OsString> {
    core::iter::once(OsString::from(FLAG)).chain(run.into_argv())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    /// **`baud -e hx /tmp/x` is that argv exactly, and nothing after `-e` is
    /// split or read as baud's**: a space inside an argument, a `--`, a
    /// second `-e` and an argument that looks like baud's own `--version`
    /// stay elements as they came; `-e` alone names no program; `-e` only
    /// counts first; and `args` gives back the arguments `asked` reads, so a
    /// click's start and the new process agree. Named break: split each
    /// argument on spaces in `asked` -- the second case has five elements.
    #[test]
    fn everything_after_dash_e_is_the_argv() {
        let argv = |v: &[&str]| match asked(os(v)) {
            Asked::Run(r) => r.into_argv(),
            other => panic!("{v:?}: {other:?}"),
        };
        assert_eq!(argv(&["-e", "hx", "/tmp/x"]), os(&["hx", "/tmp/x"]));
        assert_eq!(
            argv(&["-e", "hx", "/tmp/a b.rs", "--", "-e", "--version"]),
            os(&["hx", "/tmp/a b.rs", "--", "-e", "--version"])
        );
        assert_eq!(asked(os(&["-e"])), Asked::NoProgram);
        assert_eq!(asked(os(&["-e", ""])), Asked::NoProgram);
        assert_eq!(asked(os(&[])), Asked::Shell, "the control: no arguments");
        assert_eq!(asked(os(&["x", "-e", "hx"])), Asked::Shell, "-e only first");
        let run = crate::rules::Run::given(os(&["w3m", "https://x/?a b"])).expect("a program");
        assert_eq!(asked(args(run.clone())), Asked::Run(run));
    }
}
