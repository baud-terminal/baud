//! **The program a test runs, by name, wherever this system keeps it.** A
//! test that runs `stty`, `pwd` or `mkfifo` names the file at `/bin` or
//! `/usr/bin` where it is there, and otherwise the one the test process's own
//! `PATH` finds: a system with no programs at those two places (NixOS keeps
//! only `/bin/sh` and `/usr/bin/env` there) runs the same tests. The file at
//! the usual place is taken first, so a system that has one runs exactly
//! that file. A child's own `PATH` is left as the test sets it: the path
//! returned is absolute, so the child needs no search to find it.

use std::path::Path;

/// The absolute path of the program `name`: `/bin/<name>` or
/// `/usr/bin/<name>` where that file exists, else the first file of that
/// name in this process's `PATH`. Panics, naming the program, where none is
/// found: a test that cannot run its program says so.
pub(crate) fn tool(name: &str) -> String {
    let usual = ["/bin", "/usr/bin"].map(|d| Path::new(d).join(name));
    usual
        .into_iter()
        .chain(
            std::env::var_os("PATH")
                .iter()
                .flat_map(std::env::split_paths)
                .map(|d| d.join(name)),
        )
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| panic!("{name}: not at /bin or /usr/bin, and not on PATH"))
}
