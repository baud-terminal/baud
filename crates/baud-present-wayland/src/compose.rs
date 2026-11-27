//! **The Compose table a Linux session types with**, found where
//! libxkbcommon's `xkb_compose_table_new_from_locale()` finds it
//! (`[[source: xkbcommon-compose-h]]`, `[[source:
//! libxkbcommon-compose-paths-c]]`), in its order: `$XCOMPOSEFILE`, then
//! `$XDG_CONFIG_HOME/XCompose` (`$HOME/.config/XCompose` when that is
//! unset or relative), then `$HOME/.XCompose`, then the locale's own file
//! under the X locale directory, `$XLOCALEDIR` or `/usr/share/X11/locale`,
//! as `compose.dir` names it after `locale.alias` has resolved the locale.
//! The locale is the first of `LC_ALL`, `LC_CTYPE` and `LANG` that is set
//! and not empty, else `C`, as the header suggests for a program that does
//! not call `setlocale`. The first file that reads is the one parsed; one
//! that fails to parse makes no table rather than falling through, as in
//! the library.
//!
//! A locale `compose.dir` does not register falls back to `en_US.UTF-8`
//! only when the system has it, and otherwise makes no table, as the
//! library has since 1.12 "if missing locale detection is supported by
//! the C standard library in use": its `newlocale` is the caller's
//! `known`, so a misspelt or uninstalled locale types no Compose
//! sequences rather than another locale's.

use baud_xkb::compose::{Error, Files, Table, paths};

/// The X locale directory libxkbcommon is built with by default.
pub const XLOCALEDIR: &str = "/usr/share/X11/locale";

/// Where the table came from, and whether it parsed.
#[derive(Debug)]
pub struct Found {
    pub path: String,
    pub table: Result<Table, Error>,
}

/// The session's lookups: an environment variable, and a file's bytes.
struct Session<'a> {
    env: &'a dyn Fn(&str) -> Option<String>,
    read: &'a mut dyn FnMut(&str) -> Option<Vec<u8>>,
    known: &'a dyn Fn(&str) -> bool,
    locale: String,
}

impl Session<'_> {
    fn var(&self, name: &str) -> Option<String> {
        (self.env)(name).filter(|v| !v.is_empty())
    }

    fn xlocaledir(&self) -> String {
        self.var("XLOCALEDIR")
            .unwrap_or_else(|| XLOCALEDIR.to_string())
    }

    /// `get_locale_compose_file_path()`: the registry's file for the locale,
    /// under the X locale directory unless it is absolute.
    fn locale_path(&mut self) -> Option<String> {
        let dir = self.xlocaledir();
        let reg = (self.read)(&format!("{dir}/compose.dir"))?;
        let f = paths::locale_file(&reg, &self.locale, self.known)?;
        Some(match f.starts_with('/') {
            true => f,
            false => format!("{dir}/{f}"),
        })
    }
}

impl Files for Session<'_> {
    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        (self.read)(path)
    }
    fn home(&self) -> Option<String> {
        self.var("HOME")
    }
    fn locale_file(&mut self) -> Option<String> {
        self.locale_path()
    }
    fn system_dir(&self) -> String {
        self.xlocaledir()
    }
}

/// **The table, from the environment and the files `read` gives**, or
/// `None` when no candidate reads; `known` says whether the system has a
/// locale, which only an unregistered one asks. Pure over the three
/// lookups, so its order is tested without a file system.
pub fn find(
    env: &dyn Fn(&str) -> Option<String>,
    read: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    known: &dyn Fn(&str) -> bool,
) -> Option<Found> {
    let mut s = Session {
        env,
        read,
        known,
        locale: String::new(),
    };
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .find_map(|v| s.var(v))
        .unwrap_or_else(|| "C".to_string());
    let alias_path = format!("{}/locale.alias", s.xlocaledir());
    let alias = (s.read)(&alias_path);
    s.locale = alias
        .and_then(|a| paths::resolve(&a, paths::Match::Left, &locale))
        .unwrap_or(locale);
    let xdg = match s.var("XDG_CONFIG_HOME").filter(|d| d.starts_with('/')) {
        Some(d) => Some(format!("{d}/XCompose")),
        None => s.var("HOME").map(|h| format!("{h}/.config/XCompose")),
    };
    let candidates = [
        s.var("XCOMPOSEFILE"),
        xdg,
        s.var("HOME").map(|h| format!("{h}/.XCompose")),
    ];
    for path in candidates.into_iter().flatten() {
        if let Some(text) = (s.read)(&path) {
            return Some(Found {
                table: Table::parse(&text, &mut s),
                path,
            });
        }
    }
    let path = s.locale_path()?;
    let text = (s.read)(&path)?;
    Some(Found {
        table: Table::parse(&text, &mut s),
        path,
    })
}

/// [`find`] over the process's environment and the file system, the
/// system's locales answered by `known`.
pub fn from_session(known: &dyn Fn(&str) -> bool) -> Option<Found> {
    let env = |name: &str| std::env::var(name).ok();
    let mut read = |path: &str| std::fs::read(path).ok();
    find(&env, &mut read, known)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn run(env: &[(&str, &str)], files: &[(&str, &str)]) -> Option<(String, usize)> {
        let env: HashMap<String, String> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let files: HashMap<String, Vec<u8>> = files
            .iter()
            .map(|(k, v)| (k.to_string(), v.as_bytes().to_vec()))
            .collect();
        let get = |n: &str| env.get(n).cloned();
        let mut read = |p: &str| files.get(p).cloned();
        let f = find(&get, &mut read, &|l| l == "fr_FR.UTF-8")?;
        Some((f.path, f.table.map(|t| t.entries().len()).unwrap_or(0)))
    }

    const REG: &str = "en_US.UTF-8/Compose: en_US.UTF-8\nel_GR.UTF-8/Compose: el_GR.UTF-8\n";

    /// **The library's order, each candidate shadowing the next**:
    /// `XCOMPOSEFILE`, then the XDG file, then `~/.XCompose`, then the
    /// locale's, which `compose.dir` names after `locale.alias`.
    #[test]
    fn the_first_file_in_the_librarys_order_is_the_table() {
        let all = [
            ("/x/Compose", "<a> : \"1\"\n"),
            ("/cfg/XCompose", "<a> : \"2\"\n<b> : \"2\"\n"),
            ("/h/.config/XCompose", "<a> : \"3\"\n"),
            ("/h/.XCompose", "<a> : \"4\"\n<b> : \"4\"\n<c> : \"4\"\n"),
            ("/usr/share/X11/locale/compose.dir", REG),
            (
                "/usr/share/X11/locale/locale.alias",
                "el_GR.utf8 el_GR.UTF-8\n",
            ),
            ("/usr/share/X11/locale/el_GR.UTF-8/Compose", "<g> : \"γ\"\n"),
            ("/usr/share/X11/locale/en_US.UTF-8/Compose", "<e> : \"e\"\n"),
        ];
        let env = [
            ("XCOMPOSEFILE", "/x/Compose"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("HOME", "/h"),
            ("LANG", "el_GR.utf8"),
        ];
        assert_eq!(run(&env, &all), Some(("/x/Compose".into(), 1)));
        assert_eq!(run(&env[1..], &all), Some(("/cfg/XCompose".into(), 2)));
        // A relative XDG_CONFIG_HOME is not used: `$HOME/.config`.
        let rel = [("XDG_CONFIG_HOME", "cfg"), ("HOME", "/h")];
        assert_eq!(run(&rel, &all), Some(("/h/.config/XCompose".into(), 1)));
        assert_eq!(run(&env[2..], &all[3..]), Some(("/h/.XCompose".into(), 3)));
        assert_eq!(
            run(&env[2..], &all[4..]),
            Some(("/usr/share/X11/locale/el_GR.UTF-8/Compose".into(), 1)),
            "the alias resolves el_GR.utf8"
        );
    }

    /// The locale is `LC_ALL`, then `LC_CTYPE`, then `LANG`, an empty one
    /// skipped; none is `C`, which is `en_US.UTF-8`; an unregistered one
    /// falls back to `en_US.UTF-8` when the system has it and makes no
    /// table when it does not; `XLOCALEDIR` moves the directory.
    #[test]
    fn the_locale_picks_the_systems_file() {
        let files = [
            ("/l/compose.dir", REG),
            ("/l/el_GR.UTF-8/Compose", "<g> : \"γ\"\n"),
            ("/l/en_US.UTF-8/Compose", "<e> : \"e\"\n<f> : \"f\"\n"),
        ];
        let el = Some(("/l/el_GR.UTF-8/Compose".to_string(), 1));
        let en = Some(("/l/en_US.UTF-8/Compose".to_string(), 2));
        let d = ("XLOCALEDIR", "/l");
        assert_eq!(
            run(&[d, ("LC_ALL", "el_GR.UTF-8"), ("LANG", "C")], &files),
            el
        );
        assert_eq!(
            run(&[d, ("LC_ALL", ""), ("LC_CTYPE", "el_GR.UTF-8")], &files),
            el
        );
        assert_eq!(run(&[d, ("LANG", "el_GR.UTF-8")], &files), el);
        assert_eq!(run(&[d], &files), en, "no locale is C");
        assert_eq!(run(&[d, ("LANG", "fr_FR.UTF-8")], &files), en);
        assert_eq!(
            run(&[d, ("LANG", "tlh_KX.UTF-8")], &files),
            None,
            "a locale the system lacks"
        );
        assert_eq!(
            run(&[("LANG", "el_GR.UTF-8")], &files),
            None,
            "not /usr/share"
        );
    }

    /// A file that fails to parse is the answer, not a reason to look on.
    #[test]
    fn a_file_that_fails_is_not_passed_over() {
        let files = [
            ("/h/.XCompose", "include \"/nowhere\"\n"),
            ("/usr/share/X11/locale/compose.dir", REG),
            ("/usr/share/X11/locale/en_US.UTF-8/Compose", "<e> : \"e\"\n"),
        ];
        let got = {
            let get = |n: &str| (n == "HOME").then(|| "/h".to_string());
            let map: HashMap<&str, &str> = files.iter().copied().collect();
            let mut read = |p: &str| map.get(p).map(|t| t.as_bytes().to_vec());
            find(&get, &mut read, &|_| false).unwrap()
        };
        assert_eq!(got.path, "/h/.XCompose");
        assert!(got.table.is_err());
    }
}
