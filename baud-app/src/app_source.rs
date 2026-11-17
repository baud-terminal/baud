//! **The app's source, as its tests read it.** The app is `app.rs` and the
//! files under `src/app/` that hold its code, and a test that reads that
//! code reads it here, through one list, rather than naming a file: a check
//! that names one file passes without looking once the code it reads is in
//! another, and a negative check passes vacuously.
//!
//! A read takes one of two forms. A count, a contains or a check that
//! something is absent reads [`app_source`], every listed file in the
//! list's order. A read that cuts from one string to another, or orders two
//! strings, cuts within the one file that holds where the cut starts:
//! [`holding`], [`split_once`] and [`split`].

/// The list and each file's text, made from one list of paths so the two
/// cannot disagree: `APP_SOURCE`, the paths relative to `src/`; `APP_TEXTS`,
/// each file's text in that order; and `APP_TEXT`, those texts joined in
/// that order with nothing between them.
macro_rules! app_files {
    ($($path:literal),+ $(,)?) => {
        /// The files of the app's source, relative to `src/`, in the order
        /// [`app_source`] joins them.
        pub(crate) const APP_SOURCE: &[&str] = &[$($path),+];
        /// Each file's text, in `APP_SOURCE`'s order.
        const APP_TEXTS: &[&str] = &[$(include_str!($path)),+];
        /// Every file's text, in `APP_SOURCE`'s order.
        const APP_TEXT: &str = concat!($(include_str!($path)),+);
    };
}

app_files!("app.rs");

/// The files under `src/app/` that are not the app's source: modules of
/// their own that the app declares, which a read of the app's code leaves
/// out as it always has.
const APART: &[&str] = &[
    // The held-key checks, a test module that reads the app's code.
    "app/key_repeat.rs",
    // The launch's checks, a test module that reads the app's code.
    "app/launch_tests.rs",
    // The event rig's child, in the probe build only; its checks read it
    // by its own name.
    "app/rig.rs",
];

/// The app's whole source: every file of `APP_SOURCE`, in order.
pub(crate) const fn app_source() -> &'static str {
    APP_TEXT
}

/// The text of the first file of the app's source that holds `head`, or,
/// when none does, the whole source, so a cut from `head` finds nothing
/// there as it would in the whole: what a read cuts when it starts at
/// `head`, so the cut stays inside the file the code is in.
pub(crate) fn holding(head: &str) -> &'static str {
    APP_TEXTS
        .iter()
        .copied()
        .find(|text| text.contains(head))
        .unwrap_or(APP_TEXT)
}

/// `str::split_once` at `head`, in the file of the app's source that holds
/// it.
pub(crate) fn split_once(head: &str) -> Option<(&'static str, &'static str)> {
    holding(head).split_once(head)
}

/// `str::split` at `head`, in the file of the app's source that holds it.
pub(crate) fn split(head: &str) -> std::str::Split<'static, &str> {
    holding(head).split(head)
}

/// The text a cut from `head` in `src` runs in: when `src` is the app's
/// whole source, the file of it that holds `head`; any other text, `src`
/// itself. A helper that cuts in whatever text it is given cuts through
/// this.
pub(crate) fn within<'s>(src: &'s str, head: &str) -> &'s str {
    if src == APP_TEXT { holding(head) } else { src }
}

/// The texts a read of every occurrence in `src` cuts within: each file of
/// the app's source when `src` is the whole of it; any other text, `src`
/// alone.
pub(crate) fn texts(src: &str) -> Vec<&str> {
    if src == APP_TEXT {
        APP_TEXTS.to_vec()
    } else {
        vec![src]
    }
}

/// [`holding`] for a reading of the app's source that spells it another
/// way: `read` of the first file whose reading holds `head`, else `read` of
/// the whole source.
pub(crate) fn holding_read(read: impl Fn(&str) -> String, head: &str) -> String {
    APP_TEXTS
        .iter()
        .copied()
        .map(&read)
        .find(|text| text.contains(head))
        .unwrap_or_else(|| read(APP_TEXT))
}

/// The app's code without its tests: each file of the app's source up to
/// its test module, in the list's order. A read of "the code before the
/// tests" reads this, not the joined source cut at its last test module,
/// which once two files have one would keep every test module but the
/// last.
pub(crate) fn code() -> String {
    APP_TEXTS
        .iter()
        .map(|text| text.rsplit_once(TESTS).map_or(*text, |(code, _)| code))
        .collect()
}

/// Where a file's test module starts.
const TESTS: &str = "#[cfg(test)]\nmod tests {";

/// `str::split_once` at `head`, [`within`] `src`.
pub(crate) fn split_once_in<'s>(src: &'s str, head: &str) -> Option<(&'s str, &'s str)> {
    within(src, head).split_once(head)
}

/// `no_appkit::fn_body` of the function whose head is `head`: `head` is in
/// `src` once, and the body is cut [`within`] it.
pub(crate) fn fn_body<'s>(src: &'s str, head: &str) -> &'s str {
    assert_eq!(src.matches(head).count(), 1, "one {head}");
    crate::no_appkit::fn_body(within(src, head), head)
}

#[cfg(test)]
mod tests {
    use super::{
        APART, APP_SOURCE, APP_TEXT, APP_TEXTS, TESTS, app_source, code, fn_body, holding,
        holding_read, split, split_once, split_once_in, texts, within,
    };
    use std::path::{Path, PathBuf};

    fn src() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    /// Every `.rs` file under `dir`, as a path relative to `src/` with `/`
    /// between its parts.
    fn rust_files(dir: &Path) -> Vec<String> {
        let mut files = Vec::new();
        let mut dirs = vec![dir.to_path_buf()];
        while let Some(d) = dirs.pop() {
            for entry in std::fs::read_dir(&d).expect("the directory") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|x| x == "rs") {
                    let rel = path.strip_prefix(src()).expect("under src");
                    let parts: Vec<_> = rel.iter().map(|p| p.to_string_lossy()).collect();
                    files.push(parts.join("/"));
                }
            }
        }
        files.sort();
        files
    }

    /// The paths a list outside the crate's unit tests names: each
    /// `include_str!` of a file under `prefix`, from `from` to the `);`
    /// that closes it, with `prefix` taken off.
    fn listed<'t>(text: &'t str, from: &str, prefix: &str) -> Vec<&'t str> {
        let (_, rest) = text
            .split_once(from)
            .unwrap_or_else(|| panic!("no list at {from:?}"));
        let (list, _) = rest.split_once(");").expect("the list ends");
        let open = format!("{}(\"{prefix}", concat!("include", "_str!"));
        list.split(open.as_str())
            .skip(1)
            .map(|item| item.split_once("\")").expect("a path").0)
            .collect()
    }

    /// **The app's source is one list, and every file of the app is on
    /// it.** Every `.rs` under `src/app/` is either a file of the app's
    /// source or set apart, and not both; every file either list names
    /// exists; the app's source is `app.rs` and files under `src/app/`; each
    /// listed text is its file as it is now, and the whole is the texts in
    /// the list's order. Red with a file under `src/app/` on neither list,
    /// or a listed file deleted.
    #[test]
    fn the_code_leaves_out_every_files_tests() {
        let code = code();
        assert!(!code.contains(TESTS), "no test module is left in the code");
        for text in APP_TEXTS {
            let before = text.rsplit_once(TESTS).map_or(*text, |(c, _)| c);
            assert!(code.contains(before), "each file's code is whole in it");
        }
    }

    #[test]
    fn every_file_of_the_app_is_on_the_one_list() {
        for path in rust_files(&src().join("app")) {
            let on = APP_SOURCE.contains(&path.as_str());
            let apart = APART.contains(&path.as_str());
            assert!(
                on != apart,
                "{path}: on the app's source {on}, set apart {apart}"
            );
        }
        for path in APP_SOURCE.iter().chain(APART) {
            assert!(
                src().join(path).is_file(),
                "{path} is listed and is not a file"
            );
        }
        for path in APP_SOURCE {
            assert!(
                *path == "app.rs" || path.starts_with("app/"),
                "{path} is not the app's"
            );
        }
        for (path, text) in APP_SOURCE.iter().zip(APP_TEXTS) {
            let now = std::fs::read_to_string(src().join(path)).expect("the file");
            assert_eq!(now, *text, "{path} as the list read it");
        }
        assert_eq!(APP_TEXTS.concat(), APP_TEXT);
        assert_eq!(app_source(), APP_TEXT);
        let head = concat!("pub fn ", "run() {");
        let (_, run) = split_once(head).expect("run");
        assert!(
            holding(head).ends_with(run),
            "the cut is in the file that holds it"
        );
        assert_eq!(split(head).count(), 2, "one run");
        assert_eq!(holding(concat!("in no ", "file")), APP_TEXT);
        assert_eq!(within(APP_TEXT, head), holding(head));
        assert_eq!(within(head, head), head);
        assert_eq!(split_once_in(app_source(), head), split_once(head));
        assert_eq!(texts(app_source()), APP_TEXTS);
        assert_eq!(texts(head), [head]);
        let upper = |text: &str| text.to_uppercase();
        assert_eq!(
            holding_read(upper, &head.to_uppercase()),
            upper(holding(head))
        );
        assert!(holding(head).contains(fn_body(app_source(), head)));
    }

    /// **No unit test names a file of the app's source.** A read of the
    /// app's code goes through [`app_source`] and its cuts, so one that
    /// names `app.rs`, or a file the app is split into, is a read that
    /// misses the code once it moves. Red with a test that includes
    /// `app.rs` by its name again.
    #[test]
    fn no_test_reads_a_file_of_the_app_by_name() {
        let mut named = Vec::new();
        for path in rust_files(&src()) {
            if path == "app_source.rs" {
                continue;
            }
            let text = std::fs::read_to_string(src().join(&path)).expect("the file");
            for listed in APP_SOURCE {
                let tail = format!("{listed}\")");
                for (at, _) in text.match_indices(concat!("include", "_str!(\"")) {
                    let arg = &text[at + concat!("include", "_str!(\"").len()..];
                    let arg = arg.trim_start_matches("../");
                    if arg.starts_with(&tail) {
                        named.push(format!("{path}: {listed}"));
                    }
                }
            }
        }
        assert_eq!(named, Vec::<String>::new(), "read by name");
    }

    /// **The two reads outside the crate's unit tests list the same
    /// files.** `tests/outbox_paths.rs` and the presentation crate's
    /// `tests/cells.rs` cannot call [`app_source`], so each joins its own
    /// list of the app's files, and that list is `APP_SOURCE`, in its
    /// order. Red with a file added to `APP_SOURCE` and not to either.
    #[test]
    fn the_reads_outside_the_crate_list_the_same_files() {
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let outbox =
            std::fs::read_to_string(crate_dir.join("tests/outbox_paths.rs")).expect("outbox");
        let cells = std::fs::read_to_string(crate_dir.join("../baud-present-macos/tests/cells.rs"))
            .expect("cells");
        let titlebar =
            std::fs::read_to_string(crate_dir.join("tests/titlebar_mark.rs")).expect("titlebar");
        let frame = std::fs::read_to_string(crate_dir.join("../baud-bench/benches/frame.rs"))
            .expect("frame");
        for (name, text, from, prefix) in [
            (
                "outbox_paths.rs",
                outbox.as_str(),
                "const APP: &str = concat!(",
                "../src/",
            ),
            (
                "cells.rs",
                cells.as_str(),
                "let app = concat!(",
                "../../baud-app/src/",
            ),
            (
                "titlebar_mark.rs",
                titlebar.as_str(),
                "const APP: &str = concat!(",
                "../src/",
            ),
            (
                "frame.rs",
                frame.as_str(),
                "const APP: &str = concat!(",
                "../../baud-app/src/",
            ),
        ] {
            assert_eq!(listed(text, from, prefix), APP_SOURCE, "{name}");
        }
    }
}
