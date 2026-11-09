//! **`baud [--grid COLSxROWS] -e PROGRAM [ARGS...]`: a window running
//! PROGRAM in place of a shell**, the spelling `kitty -e`, `alacritty -e`
//! and `wezterm start --` give it. It is how a link's `action = "window"`
//! opens a window on Linux, whose driver runs one window per process: the
//! click starts this binary again with `-e` and the rule's argv after it,
//! and `--grid` with the clicking window's grid, so the new window opens at
//! the size of the one it was asked from (`linux.rs`). Where it goes on the
//! screen, and which window has the keyboard when it closes, are the
//! compositor's: Wayland lets no client place itself. Pure over the
//! arguments, so its tests run on every platform.
//!
//! **Everything after `-e` is the argv, never re-split**: each argument is
//! one element as the caller's `execve` passed it, a space, a quote or a
//! `--` inside it included, and none is read as an option of baud's. `-e`
//! is read only as the first argument, or the third after `--grid` and its
//! value, as `--version` is read first, so nothing before it is skipped
//! unread.
#![forbid(unsafe_code)]

use std::ffi::OsString;

use baud_domain::{Cols, Rows, WinSize};

/// The flag.
pub(crate) const FLAG: &str = "-e";

/// The grid's flag, before [`FLAG`]: `COLSxROWS`, the `BAUD_GRID` shape.
pub(crate) const GRID: &str = "--grid";

/// What the arguments ask.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Asked {
    /// No `-e`: a shell window, as a launch with no arguments.
    Shell,
    /// `-e` and the argv after it, at the grid `--grid` gave, if it did.
    Run(crate::rules::Run, Option<Grid>),
    /// `-e` with no program after it, which is said and refused.
    NoProgram,
    /// `--grid` with no `COLSxROWS` after it, or none `-e` follows.
    BadGrid,
}

/// **A window's grid**, columns by rows, as `--grid COLSxROWS` names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Grid {
    cols: Cols,
    rows: Rows,
}

impl Grid {
    /// The grid of `cols` by `rows`.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub(crate) fn of(cols: Cols, rows: Rows) -> Grid {
        Grid { cols, rows }
    }

    /// As the pty and the core take it, each side at most `max`'s.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub(crate) fn within(self, max: &WinSize) -> WinSize {
        let side = |n: usize, m: u16| u16::try_from(n).unwrap_or(u16::MAX).min(m);
        WinSize {
            ws_col: side(self.cols.index(), max.ws_col),
            ws_row: side(self.rows.index(), max.ws_row),
        }
    }

    /// `COLSxROWS`, each at least one; none for anything else.
    fn read(v: &std::ffi::OsStr) -> Option<Grid> {
        let (c, r) = v.to_str()?.split_once('x')?;
        let (c, r): (u16, u16) = (c.parse().ok()?, r.parse().ok()?);
        (c > 0 && r > 0).then(|| Grid {
            cols: Cols::of_index(usize::from(c)),
            rows: Rows::of_index(usize::from(r)),
        })
    }

    /// `COLSxROWS`, as [`Grid::read`] reads it.
    fn written(self) -> OsString {
        OsString::from(format!("{}x{}", self.cols.index(), self.rows.index()))
    }
}

/// **What `args`, the arguments after the program's own name, ask**, with
/// a program after `-e` that is a path relative to a folder made absolute
/// against `here`, the folder baud was started in ([`typed_at`]).
pub(crate) fn asked<I: IntoIterator<Item = OsString>>(
    args: I,
    here: Option<&std::path::Path>,
) -> Asked {
    let mut args = args.into_iter();
    let mut first = args.next();
    let mut size = None;
    if first.as_deref().is_some_and(|f| f == GRID) {
        match (args.next().as_deref().and_then(Grid::read), args.next()) {
            (Some(ws), Some(e)) if e == FLAG => {
                size = Some(ws);
                first = Some(e);
            }
            _ => return Asked::BadGrid,
        }
    }
    match first {
        Some(first) if first == FLAG => {
            match crate::rules::Run::given(typed_at(args.collect(), here)) {
                Some(run) => Asked::Run(run, size),
                None => Asked::NoProgram,
            }
        }
        _ => Asked::Shell,
    }
}

/// **A program typed as a path relative to a folder is made absolute where
/// it was typed**, against `here`, once, before any window starts: the
/// child changes folder before it runs its program, so the exec door
/// takes an absolute path or a bare name only (`pty`'s `ExecPath`), and
/// `./x` reaching it would not be found. `.` components are dropped and
/// `..` kept, since a symbolic link before it decides where it leads. A
/// bare name stays for `PATH`; with no `here` the program stays as typed
/// and is not found.
fn typed_at(mut argv: Vec<OsString>, here: Option<&std::path::Path>) -> Vec<OsString> {
    use std::os::unix::ffi::OsStrExt;
    if let (Some(first), Some(here)) = (argv.first_mut(), here) {
        let typed = std::path::Path::new(first.as_os_str());
        if first.as_bytes().contains(&b'/') && typed.is_relative() {
            *first = here
                .join(typed)
                .components()
                .collect::<std::path::PathBuf>()
                .into_os_string();
        }
    }
    argv
}

/// **The arguments that start a window running `run`** at `size`:
/// [`GRID`] and the grid when there is one, [`FLAG`], then the argv's
/// elements, each one argument, moved.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) fn args(run: crate::rules::Run, size: Option<Grid>) -> impl Iterator<Item = OsString> {
    let grid = size.map(|g| [OsString::from(GRID), g.written()]);
    grid.into_iter()
        .flatten()
        .chain(core::iter::once(OsString::from(FLAG)))
        .chain(run.into_argv())
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
        let argv = |v: &[&str]| match asked(os(v), None) {
            Asked::Run(r, None) => r.into_argv(),
            other => panic!("{v:?}: {other:?}"),
        };
        assert_eq!(argv(&["-e", "hx", "/tmp/x"]), os(&["hx", "/tmp/x"]));
        assert_eq!(
            argv(&["-e", "hx", "/tmp/a b.rs", "--", "-e", "--version"]),
            os(&["hx", "/tmp/a b.rs", "--", "-e", "--version"])
        );
        assert_eq!(asked(os(&["-e"]), None), Asked::NoProgram);
        assert_eq!(asked(os(&["-e", ""]), None), Asked::NoProgram);
        assert_eq!(
            asked(os(&[]), None),
            Asked::Shell,
            "the control: no arguments"
        );
        assert_eq!(
            asked(os(&["x", "-e", "hx"]), None),
            Asked::Shell,
            "-e only first"
        );
        let run = crate::rules::Run::given(os(&["w3m", "https://x/?a b"])).expect("a program");
        assert_eq!(asked(args(run.clone(), None), None), Asked::Run(run, None));
    }

    /// **`--grid COLSxROWS` before `-e` gives the new window the clicked
    /// window's grid**, and `args` writes it so the new process reads it
    /// back; a `--grid` with no grid, a zero, or no `-e` after it is refused,
    /// and `--grid` anywhere but first is an argument of the program's; the
    /// pty takes it within what baud provisions.
    /// Named break: leave the grid out of `args` -- the new window opens at
    /// the default grid.
    #[test]
    fn a_links_window_on_linux_takes_the_clicked_grid() {
        let ws = Grid::of(Cols::of_index(132), Rows::of_index(43));
        let run = crate::rules::Run::given(os(&["hx", "/tmp/x"])).expect("a program");
        let given: Vec<OsString> = args(run.clone(), Some(ws)).collect();
        assert_eq!(given, os(&["--grid", "132x43", "-e", "hx", "/tmp/x"]));
        assert_eq!(asked(given, None), Asked::Run(run, Some(ws)));
        for bad in [
            &["--grid"][..],
            &["--grid", "132x43"],
            &["--grid", "0x43", "-e", "hx"],
            &["--grid", "132", "-e", "hx"],
            &["--grid", "132x43", "hx"],
        ] {
            assert_eq!(asked(os(bad), None), Asked::BadGrid, "{bad:?}");
        }
        let inside = crate::rules::Run::given(os(&["hx", "--grid", "1x1"])).expect("a program");
        assert_eq!(
            asked(os(&["-e", "hx", "--grid", "1x1"]), None),
            Asked::Run(inside, None)
        );
        // The pty's grid, each side within what baud provisions.
        let max = WinSize {
            ws_col: 500,
            ws_row: 200,
        };
        let pty = ws.within(&max);
        assert_eq!((pty.ws_col, pty.ws_row), (132, 43));
        let pty = Grid::of(Cols::of_index(9999), Rows::of_index(3)).within(&max);
        assert_eq!((pty.ws_col, pty.ws_row), (500, 3), "clamped");
    }

    /// **`-e ./x` is made absolute where it was typed**, against the folder
    /// baud was started in, before any window starts: the child changes
    /// folder before it runs the program, so a relative path would name a
    /// file there instead, and the exec door refuses one. `a/b` and `../x`
    /// alike; the arguments after the program are as they came, and a bare
    /// name or an absolute path is as written, the controls.
    #[test]
    fn a_relative_program_is_made_absolute_where_it_was_typed() {
        let here = std::path::Path::new("/launch/dir");
        let program = |args: &[&str]| match asked(os(args), Some(here)) {
            Asked::Run(run, _) => (run.program().to_os_string(), run.argv().to_vec()),
            other => panic!("{args:?}: {other:?}"),
        };
        let at = |p: &str| here.join(p).components().collect::<std::path::PathBuf>();
        for typed in ["./x", "a/b", "../x"] {
            let (got, argv) = program(&["-e", typed, "./arg"]);
            assert_eq!(got, at(typed).into_os_string(), "{typed}");
            assert_eq!(argv[1], "./arg", "{typed}: its arguments as they came");
        }
        assert_eq!(program(&["-e", "hx"]).0, "hx", "a bare name, the control");
        assert_eq!(program(&["-e", "/bin/sh"]).0, "/bin/sh", "an absolute path");
        assert!(
            matches!(asked(os(&["-e", "./x"]), None), Asked::Run(r, _) if r.program() == "./x"),
            "with no folder known it stays as typed, and the exec door finds nothing"
        );
    }
}
