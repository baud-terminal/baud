//! **The build's name, where the app says it**: `<package version>+<hash>`,
//! what `baud --version` prints, the title bar shows, `TERM_PROGRAM_VERSION`
//! exports and XTVERSION names. `build.rs` writes it into `BAUD_VERSION`; a
//! manifest never holds a hash.

use baud_domain::VersionText;

/// `<package version>+<identifier>`, this build's.
pub(crate) const BUILD: &str = env!("BAUD_VERSION");

/// **When this build's commit was made**, as `build.rs` stamps it: whole
/// seconds since 1970, or empty with no commit time to give.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const RELEASED: &str = env!("BAUD_RELEASED");

/// **What the About window's age line counts from**: `stamp`'s time, unless
/// `build` names no commit (`+unknown`, or no `+` at all) or the stamp holds no time, and then
/// no age line at all, rather than an age nothing backs.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn released(build: &str, stamp: &str) -> Option<std::time::SystemTime> {
    let secs = Some(stamp.trim())
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))?
        .parse()
        .ok()
        .filter(|&n| n != 0)?;
    match build.rsplit_once('+') {
        Some((_, "unknown")) | None => None,
        Some(_) => {
            std::time::SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(secs))
        }
    }
}

/// **The text XTVERSION names**: a build whose name the core would refuse,
/// past xterm's 80 bytes or with a byte that could end the control string,
/// fails here at compile time rather than answer with the wrong version.
pub(crate) static TEXT: VersionText = match VersionText::new(BUILD) {
    Some(text) => text,
    None => panic!("BAUD_VERSION is not a text XTVERSION can carry"),
};

/// **`baud --version`**: only the first argument, and only that spelling.
/// Everything else starts the app as it always has.
pub(crate) fn asks<I: IntoIterator<Item = std::ffi::OsString>>(args: I) -> bool {
    args.into_iter()
        .next()
        .is_some_and(|first| first == "--version")
}

/// **What `baud --version` prints first**: the name and the build's name,
/// the first line GNU's coding standards give `--version`, which "is meant
/// to be easy for a program to parse; the version number proper starts
/// after the last space" (`[[source: gnu-coding-standards-version]]`).
pub(crate) fn line(build: &str) -> String {
    format!("baud {build}")
}

/// **baud's copyright and licence, as the About window states them**:
/// `LICENSE`'s holder and its licence, which `crates/baud-app/tests/about_window.rs`
/// holds to that file. The one source of both: the About window shows it
/// whole, and `baud --version` prints its two halves on lines of their own.
pub(crate) const COPYRIGHT: &str = "Copyright \u{a9} 2026 i-am-logger \u{b7} CC BY-NC-SA 4.0";

/// What parts [`COPYRIGHT`]'s copyright notice from its licence.
const BETWEEN: &str = " \u{b7} ";

/// Where `baud --version` points for the third-party notices.
pub(crate) const CREDITS_POINTER: &str = "Third-party notices: baud --credits";

/// **[`COPYRIGHT`]'s two halves**: the copyright notice, then the licence,
/// each the About window's own words.
pub(crate) fn notice() -> (&'static str, &'static str) {
    COPYRIGHT.split_once(BETWEEN).unwrap_or((COPYRIGHT, ""))
}

/// **All `baud --version` prints**, as GNU's coding standards order it
/// (`[[source: gnu-coding-standards-version]]`): the [`line`], unchanged,
/// then "a copyright notice", then "a line stating the license", each from
/// [`COPYRIGHT`], and where the third-party notices are. Nothing else
/// prints any of it: a start that is not `--version` prints none of these.
pub(crate) fn text(build: &str) -> String {
    let (copyright, licence) = notice();
    format!(
        "{}\n{copyright}\n{licence}\n{CREDITS_POINTER}\n",
        line(build)
    )
}

/// **The About window's version line**: "Version 1.0.0 (58.4)", as AppKit's
/// standard panel writes a marketing version and a build version
/// (`[[source: sdk27-nsapplication-h]]`, `NSAboutPanelOptionVersion`), the
/// build version being what follows the `+`. With no `+`, the version alone.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn about_line(build: &str) -> String {
    match build.split_once('+') {
        Some((version, id)) => format!("Version {version} ({id})"),
        None => format!("Version {build}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn asked(args: &[&str]) -> bool {
        asks(args.iter().map(OsString::from))
    }

    /// **Only `--version` as the first argument prints and leaves.** Red
    /// first: `main` starts the app on any arguments it does not know, which
    /// no test may run; so this and the source check below hold it, and no
    /// test launches the binary. Perturbation: `asks` answering for any
    /// argument fails the second half.
    #[test]
    fn only_version_first_asks() {
        assert!(asked(&["--version"]));
        assert!(asked(&["--version", "extra"]));
        assert!(!asked(&[]));
        assert!(!asked(&["-V"]));
        assert!(!asked(&["--versions"]));
        assert!(!asked(&["-psn_0_12345"]));
        assert!(!asked(&["extra", "--version"]));
    }

    /// **`main` answers before anything is set up**: before the startup
    /// clock, the font thread and either driver, so `--version` opens no
    /// window. In the source, since running it to find out is the failure
    /// itself. Perturbation: move the check below `app::run()` and this
    /// fails.
    #[test]
    fn main_answers_before_it_starts_anything() {
        const SRC: &str = include_str!("main.rs");
        let body = &SRC[SRC.find(concat!("fn ", "main() {")).expect("main")..];
        let asked = body.find(concat!("version::", "asks(")).expect("main asks");
        for later in [
            concat!("host::", "startup_begin()"),
            concat!("log::", "init()"),
            concat!("app::", "run()"),
            concat!("linux::", "run("),
        ] {
            assert!(
                asked < body.find(later).expect(later),
                "the answer comes before {later}"
            );
        }
    }

    /// **The line printed and the line About shows, from this build's name
    /// and from a named one.**
    #[test]
    fn the_lines_say_the_build() {
        assert_eq!(
            line("1.0.0-unstable+abc1234"),
            "baud 1.0.0-unstable+abc1234"
        );
        assert_eq!(line(BUILD), format!("baud {BUILD}"));
        assert_eq!(
            about_line("1.0.0-unstable+abc1234.dirty"),
            "Version 1.0.0-unstable (abc1234.dirty)"
        );
        assert_eq!(
            about_line("1.0.0-unstable.2+abc1234"),
            "Version 1.0.0-unstable.2 (abc1234)"
        );
        assert_eq!(about_line("1.0.0+unknown"), "Version 1.0.0 (unknown)");
        assert_eq!(about_line("1.0.0"), "Version 1.0.0");
        assert!(
            BUILD.starts_with(concat!(env!("CARGO_PKG_VERSION"), "+")),
            "{BUILD}: the package version, then the metadata"
        );
    }

    /// **`--version` is its first line, unchanged, then the About window's
    /// copyright notice and licence, the same constant, then the pointer to
    /// the third-party notices.** Perturbation: a licence retyped here
    /// rather than taken from [`COPYRIGHT`] fails the join; the pointer
    /// dropped fails its line.
    #[test]
    fn version_says_the_copyright_the_licence_and_where_credits_are() {
        let text = text("1.0.0-unstable+abc1234");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0], "baud 1.0.0-unstable+abc1234",
            "the first line, as scripts read it"
        );
        assert_eq!(lines[0], line("1.0.0-unstable+abc1234"));
        let (copyright, licence) = notice();
        assert_eq!(lines[1], copyright);
        assert_eq!(lines[2], licence);
        assert_eq!(
            format!("{}{BETWEEN}{}", lines[1], lines[2]),
            COPYRIGHT,
            "the two lines are the About window's one"
        );
        assert!(lines[1].starts_with("Copyright \u{a9} "), "{}", lines[1]);
        assert!(!lines[2].is_empty(), "a licence line");
        assert_eq!(lines[3], CREDITS_POINTER);
        assert_eq!(lines.len(), 4);
        assert!(text.ends_with('\n'));
    }

    /// **A start that is not `--version` or `--credits` prints none of
    /// it**: the version text and the notices are printed only from
    /// `main`'s three early answers, version, credits and usage, each of
    /// which returns, and nothing else
    /// in the app names them to print. In the source, as the early answers
    /// are held, since a normal start opens a window. Perturbation: print
    /// `version::text` after `log::init()`, or call `credits::print`
    /// anywhere else, and this fails.
    #[test]
    fn a_normal_start_prints_no_version_or_credits() {
        const SRC: &str = include_str!("main.rs");
        let body = &SRC[SRC.find(concat!("fn ", "main() {")).expect("main")..];
        let started = body
            .find(concat!("host::", "startup_begin()"))
            .expect("the start");
        let (early, rest) = body.split_at(started);
        for name in [
            concat!("version::", "text("),
            concat!("credits::", "print("),
            concat!("credits::", "TEXT"),
            concat!("CREDITS_", "POINTER"),
        ] {
            assert!(!rest.contains(name), "{name} after the start");
        }
        assert_eq!(early.matches(concat!("version::", "text(")).count(), 1);
        assert_eq!(early.matches(concat!("credits::", "print(")).count(), 1);
        assert_eq!(
            early.matches(concat!("        ", "return;")).count(),
            3,
            "each early answer returns"
        );
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![dir.clone()];
        let mut read = 0;
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).expect("src") {
                let p = e.expect("an entry").path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !name.ends_with(".rs") || ["main.rs", "version.rs", "credits.rs"].contains(&name)
                {
                    continue;
                }
                let src = std::fs::read_to_string(&p).expect("a source");
                read += 1;
                for name in [
                    concat!("version::", "text("),
                    concat!("credits::", "print("),
                    concat!("version::", "CREDITS_POINTER"),
                ] {
                    assert!(!src.contains(name), "{}: names {name}", p.display());
                }
                // The notices are printed only by --credits.
                assert!(
                    !src.contains(concat!("credits::", "TEXT")),
                    "{}: names the notices",
                    p.display()
                );
            }
        }
        assert!(read > 50, "the control: only {read} files read");
    }

    /// **An age line only for a commit with a time**: `+unknown` names no
    /// commit, so a time stamped beside it is not shown, and an empty, zero
    /// or broken stamp is no time. Perturbation: drop the `unknown` arm and
    /// the first `None` fails; read an empty stamp as zero seconds and the
    /// second does.
    #[test]
    fn no_age_without_a_commit_and_its_time() {
        let at = |secs| std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        assert_eq!(released("1.0.0+unknown", "1794042906"), None);
        assert_eq!(released("1.0.0-unstable+0123abc", ""), None);
        assert_eq!(released("1.0.0-unstable+0123abc", "0"), None);
        assert_eq!(released("1.0.0-unstable+0123abc", "-5"), None);
        assert_eq!(released("1.0.0-unstable+0123abc", "17940x"), None);
        assert_eq!(released("1.0.0", "1794042906"), None);
        assert_eq!(
            released("1.0.0-unstable+0123abc", "1794042906"),
            Some(at(1_794_042_906))
        );
        assert_eq!(
            released("1.0.0-unstable+0123abc.dirty", "1794042906"),
            Some(at(1_794_042_906))
        );
    }
}
