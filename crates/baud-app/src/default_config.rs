//! **The commented default config, written once where none exists.**
//!
//! When no file is at any place the config is looked up (`config::lookup`)
//! and `BAUD_CONFIG` names none, a launch writes [`WRITTEN`] -- a line
//! naming the baud that wrote it, then `config.toml.example` byte for byte
//! -- to `$XDG_CONFIG_HOME/baud/config.toml`, or `~/.config/baud/
//! config.toml` with that unset, before the config is read, so the watch
//! takes it as the file in force. Every key in it is commented out
//! (`build.rs` stops on a line in force), so it changes nothing; a later
//! baud never rewrites it, and the user's editor is the only thing that
//! changes it after.
//!
//! * **Never over anything.** The file is written to a temporary name in
//!   its directory and then linked to its own name, which fails when
//!   anything is there, so two windows starting at once cannot clobber each
//!   other and a file made meanwhile is kept. Any entry at a lookup place
//!   counts as there, a link that names nothing included -- home-manager's,
//!   between generations -- and so does one that cannot be looked at.
//! * **Never for `BAUD_CONFIG`.** It names a file the user chose, so with
//!   it set nothing is written anywhere.
//! * **A failure is a line in the log, once**: a read-only home, a place
//!   that is not a directory, or no `HOME` at all. The launch goes on with
//!   the defaults. It is not a fault in the user's config, so no row in the
//!   window says it.
//! * **A launch killed while it writes** leaves its temporary,
//!   `.config.toml.<pid>.new`, beside the config. Nothing removes it but a
//!   later launch with the same process id, which removes only its own
//!   name: baud deletes no other file in the user's directory.
//! * **An empty file stays**: it is there, so nothing is written, and an
//!   empty config is the defaults. That is how to have no written file.
//!
//! [`write_if_none`] takes the three lookup inputs as arguments, as
//! `config::config_paths` does, so a test writes only under a directory of
//! its own; only [`at_launch`] reads the environment, and only the two
//! drivers' `run` call it, once a window is to open.

use std::path::{Path, PathBuf};

/// The example config, compiled in.
#[cfg(test)]
pub(crate) const EXAMPLE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/config.toml.example"));

/// **What is written**: the baud that wrote it, then the example.
pub(crate) const WRITTEN: &str = concat!(
    "# Written by baud ",
    env!("BAUD_VERSION"),
    ", which found no config file.\n",
    "# Remove this file and the next launch writes the current one.\n",
    "\n",
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/config.toml.example"))
);

/// What a launch did about the default config.
#[derive(Debug)]
pub(crate) enum Wrote {
    /// `BAUD_CONFIG` names the config: nothing is written.
    Chosen,
    /// A lookup place has an entry: the config is the user's.
    Present,
    /// No absolute `XDG_CONFIG_HOME` or `HOME`: nowhere to write.
    Nowhere,
    /// Written, at this path.
    Written(PathBuf),
    /// Another launch linked its file to the name first, which stands.
    Raced(PathBuf),
    /// Not written, at this path, for this reason.
    Failed(PathBuf, std::io::Error),
}

/// **Whether an entry is at `path`**: anything but "no such entry" is one,
/// a dangling link and one that cannot be looked at included, so a path
/// baud cannot see is never written over. A path through a file that is not
/// a directory (`~/.config` a file) names no entry either: nothing can be at
/// it, so the write is tried and its failure said.
fn there(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(e) => !matches!(
            e.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
        ),
    }
}

/// **The default config written when none is there**, from the lookup's
/// inputs: `given`, `BAUD_CONFIG`; `xdg` and `home`, each already held to
/// an absolute path (`config::absolute_dir`). It goes to the first of the
/// lookup's own places, XDG's when it is set.
pub(crate) fn write_if_none(
    given: Option<PathBuf>,
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Wrote {
    if given.is_some() {
        return Wrote::Chosen;
    }
    let places = crate::config::config_paths(None, xdg, home);
    if places.iter().any(|p| there(p)) {
        return Wrote::Present;
    }
    let Some(target) = places.into_iter().next() else {
        return Wrote::Nowhere;
    };
    match write_new(&target, WRITTEN.as_bytes()) {
        Ok(Linked::Made) => Wrote::Written(target),
        Ok(Linked::Taken) => Wrote::Raced(target),
        Err(e) => Wrote::Failed(target, e),
    }
}

/// Whether the link took the name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Linked {
    Made,
    /// Something was at the name already; it is left as it was.
    Taken,
}

/// **`bytes` at `target`, only where nothing is**: written in full to a
/// temporary name in its directory, then linked to the name, which
/// fails when anything is there (`the_link_never_writes_over_a_file`); the
/// temporary is removed either way. The directories are made as XDG's specification asks, "If,
/// when attempting to write a file, the destination directory is
/// non-existent an attempt should be made to create it with permission
/// 0700" (`[[source: xdg-basedir-0-8]]`).
fn write_new(target: &Path, bytes: &[u8]) -> std::io::Result<Linked> {
    use std::io::Write;
    use std::os::unix::fs::DirBuilderExt;
    let dir = target.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    // This process's own name: no other process alive has its id, so one
    // left there is from an earlier process that stopped before removing
    // it.
    let temporary = dir.join(format!(".config.toml.{}.new", std::process::id()));
    let _ = std::fs::remove_file(&temporary);
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        });
    let linked = written.and_then(|()| match std::fs::hard_link(&temporary, target) {
        Ok(()) => Ok(Linked::Made),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(Linked::Taken),
        Err(e) => Err(e),
    });
    let _ = std::fs::remove_file(&temporary);
    linked
}

/// **The default config written at a launch**, from the environment, and
/// what happened said in the log: a write at `info`, a failure at the
/// default `warn`, once, since a launch calls it once.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn at_launch() {
    let given = std::env::var_os("BAUD_CONFIG")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let dir = |k: &str| crate::config::absolute_dir(std::env::var_os(k));
    let rest = "baud starts with the defaults";
    match write_if_none(given, dir("XDG_CONFIG_HOME"), dir("HOME")) {
        Wrote::Written(p) => crate::info!(
            "config: none found; wrote the commented defaults to {}",
            p.display()
        ),
        Wrote::Failed(p, e) => crate::warn!(
            "config: none found, and the commented defaults could not be written to {} ({e}); {rest}",
            p.display()
        ),
        Wrote::Nowhere => crate::warn!(
            "config: none found, and no absolute HOME or XDG_CONFIG_HOME to write one under; {rest}"
        ),
        Wrote::Raced(p) => crate::info!(
            "config: another launch wrote {} first, and it stands",
            p.display()
        ),
        Wrote::Chosen | Wrote::Present => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory of this test's own, under the temporary directory
    /// and never the home: nothing here writes anywhere else.
    fn scratch(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("baud-default-config-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a scratch directory");
        d
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut n: Vec<String> = std::fs::read_dir(dir)
            .map(|r| {
                r.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        n.sort();
        n
    }

    /// **With nothing anywhere, the file goes to `~/.config/baud`**, whole,
    /// and no temporary is left beside it. The written bytes are the
    /// version line and the example. Perturbation: write somewhere else, or
    /// leave the temporary.
    #[test]
    fn with_none_anywhere_the_defaults_are_written_under_home() {
        let home = scratch("home");
        let got = write_if_none(None, None, Some(home.clone()));
        let at = home.join(".config/baud/config.toml");
        assert!(matches!(&got, Wrote::Written(p) if *p == at), "{got:?}");
        let text = std::fs::read_to_string(&at).expect("the file");
        assert_eq!(text, WRITTEN);
        assert!(text.ends_with(EXAMPLE), "the example, byte for byte");
        assert!(
            text.starts_with(concat!("# Written by baud ", env!("BAUD_VERSION"))),
            "the version it came from"
        );
        assert_eq!(names(at.parent().unwrap()), ["config.toml"]);
        let _ = std::fs::remove_dir_all(home);
    }

    /// **`XDG_CONFIG_HOME` set, it goes there**, and the home is left
    /// alone. Perturbation: always write under the home.
    #[test]
    fn with_xdg_set_the_defaults_go_there() {
        let xdg = scratch("xdg");
        let home = scratch("xdg-home");
        let got = write_if_none(None, Some(xdg.clone()), Some(home.clone()));
        let at = xdg.join("baud/config.toml");
        assert!(matches!(&got, Wrote::Written(p) if *p == at), "{got:?}");
        assert!(names(&home).is_empty(), "the home is untouched");
        let _ = std::fs::remove_dir_all(xdg);
        let _ = std::fs::remove_dir_all(home);
    }

    /// **`BAUD_CONFIG` set, nothing is written anywhere**, even when its
    /// file is missing. Perturbation: write the default with it set.
    #[test]
    fn baud_config_set_writes_nothing() {
        let home = scratch("given");
        let got = write_if_none(Some(home.join("missing.toml")), None, Some(home.clone()));
        assert!(matches!(got, Wrote::Chosen), "{got:?}");
        assert!(names(&home).is_empty());
        let _ = std::fs::remove_dir_all(home);
    }

    /// **A file at any lookup place is the user's**: one under the home
    /// stops a write to XDG's place, and an empty file stays empty, the
    /// defaults. Perturbation: look only at the first place, or read an
    /// empty file as none.
    #[test]
    fn a_file_anywhere_in_the_lookup_stops_the_write() {
        let xdg = scratch("present-xdg");
        let home = scratch("present-home");
        let mine = home.join(".config/baud/config.toml");
        std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
        std::fs::write(&mine, "").unwrap();
        let got = write_if_none(None, Some(xdg.clone()), Some(home.clone()));
        assert!(matches!(got, Wrote::Present), "{got:?}");
        assert!(names(&xdg).is_empty(), "nothing made under XDG's place");
        assert_eq!(
            std::fs::read_to_string(&mine).unwrap(),
            "",
            "it stays empty"
        );
        let _ = std::fs::remove_dir_all(xdg);
        let _ = std::fs::remove_dir_all(home);
    }

    /// **A link that names nothing counts as there**, as home-manager's
    /// does between generations, and is left as it was. The control is
    /// the same home with no link, which is written. Perturbation: test
    /// with `exists()`, which follows the link.
    #[test]
    fn a_dangling_link_counts_as_there() {
        let home = scratch("dangling");
        let at = home.join(".config/baud/config.toml");
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(home.join("nowhere"), &at).unwrap();
        let got = write_if_none(None, None, Some(home.clone()));
        assert!(matches!(got, Wrote::Present), "{got:?}");
        assert_eq!(std::fs::read_link(&at).unwrap(), home.join("nowhere"));
        std::fs::remove_file(&at).unwrap();
        let control = write_if_none(None, None, Some(home.clone()));
        assert!(matches!(control, Wrote::Written(_)), "{control:?}");
        let _ = std::fs::remove_dir_all(home);
    }

    /// **The link never writes over what is there**: a file made at the
    /// name before the link is kept, the write says it lost the race, and
    /// the temporary is gone. Perturbation: rename the temporary over the
    /// name.
    #[test]
    fn the_link_never_writes_over_a_file() {
        let dir = scratch("race");
        let at = dir.join("config.toml");
        std::fs::write(&at, "font_size = 20\n").unwrap();
        assert_eq!(write_new(&at, b"# mine\n").unwrap(), Linked::Taken);
        assert_eq!(std::fs::read_to_string(&at).unwrap(), "font_size = 20\n");
        assert_eq!(names(&dir), ["config.toml"], "no temporary left");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// **A place that cannot be written fails, and says where**: a home
    /// that is a file, so its `.config` cannot be made whoever runs the
    /// test. No home at all is nowhere. Perturbation: report a failure as
    /// written.
    #[test]
    fn a_home_that_cannot_hold_the_file_fails() {
        let dir = scratch("file-home");
        let home = dir.join("home");
        std::fs::write(&home, "").unwrap();
        let got = write_if_none(None, None, Some(home.clone()));
        assert!(
            matches!(&got, Wrote::Failed(p, _) if *p == home.join(".config/baud/config.toml")),
            "{got:?}"
        );
        assert!(matches!(write_if_none(None, None, None), Wrote::Nowhere));
        let _ = std::fs::remove_dir_all(dir);
    }
}
