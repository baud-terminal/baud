//! **The build's name, where the app says it**: `<package version>+<hash>`,
//! what `baud --version` prints, the title bar shows, `TERM_PROGRAM_VERSION`
//! exports and XTVERSION names. `build.rs` writes it into `BAUD_VERSION`; a
//! manifest never holds a hash.

use baud_domain::VersionText;

/// `<package version>+<identifier>`, this build's.
pub(crate) const BUILD: &str = env!("BAUD_VERSION");

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

/// **What `baud --version` prints**: the name and the build's name, the
/// first line GNU's coding standards give `--version`.
pub(crate) fn line(build: &str) -> String {
    format!("baud {build}")
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
            concat!("linux::", "run()"),
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
        assert_eq!(line("1.0.0-beta.1+abc1234"), "baud 1.0.0-beta.1+abc1234");
        assert_eq!(line(BUILD), format!("baud {BUILD}"));
        assert_eq!(
            about_line("1.0.0-beta.1+abc1234.dirty"),
            "Version 1.0.0-beta.1 (abc1234.dirty)"
        );
        assert_eq!(about_line("1.0.0+unknown"), "Version 1.0.0 (unknown)");
        assert_eq!(about_line("1.0.0"), "Version 1.0.0");
        assert!(
            BUILD.starts_with(concat!(env!("CARGO_PKG_VERSION"), "+")),
            "{BUILD}: the package version, then the metadata"
        );
    }
}
