//! **`demo-open`, the tour's opener for a path baud's link was clicked
//! on**: a GIF plays in the player yazi's opener runs, `kitten icat
//! --hold`, and any other file opens in helix. One path in, one program
//! run in its place by `exec` with an argument vector: no shell reads the
//! path, so a space, a quote or a `$` in it is the path's own. A relative
//! path that starts with `-` is given as `./-...`, the same file, so no
//! program reads it as one of its options.
//!
//! ```text
//! demo-open docs/assets/readme/logo-dark.gif   # kitten icat --hold docs/assets/readme/logo-dark.gif
//! demo-open README.md                     # hx README.md
//! ```

use std::ffi::{OsStr, OsString};
use std::path::Path;

/// **The program and its arguments for `path`**: `kitten icat --hold`
/// for a `.gif`, the extension in any case, and `hx` for anything else,
/// the path last and whole.
fn argv(path: &OsStr) -> Vec<OsString> {
    let shown = match path.as_encoded_bytes().first() {
        Some(b'-') => Path::new(".").join(path).into_os_string(),
        _ => path.to_os_string(),
    };
    let gif = Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gif"));
    let mut a: Vec<OsString> = match gif {
        true => ["kitten", "icat", "--hold"].map(OsString::from).to_vec(),
        false => vec![OsString::from("hx")],
    };
    a.push(shown);
    a
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    use std::os::unix::process::CommandExt as _;
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let [path] = args.as_slice() else {
        eprintln!("demo-open: give one path");
        return std::process::ExitCode::from(2);
    };
    let a = argv(path);
    let err = std::process::Command::new(&a[0]).args(&a[1..]).exec();
    eprintln!("demo-open: {}: {err}", a[0].to_string_lossy());
    std::process::ExitCode::from(127)
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!("demo-open: the tour's opener runs on macOS");
    std::process::ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(a: &[OsString]) -> Vec<&str> {
        a.iter().map(|s| s.to_str().unwrap()).collect()
    }

    /// **A GIF goes to kitten and anything else to helix, the path one
    /// argument**: `.gif` and `.GIF` to `kitten icat --hold`, a `.md` and
    /// a file with no extension to `hx`; a path with spaces, a quote and a
    /// `$` stays one argument, as it is; a leading `-` becomes `./-`, the
    /// same file, still one argument. The control: `.gift` is no GIF.
    /// Perturbation: split the path on spaces and the spaces' assertion
    /// fails.
    #[test]
    fn a_gif_goes_to_kitten_and_the_rest_to_helix() {
        let at = |p: &str| strs(&argv(OsStr::new(p))).join("\u{1}");
        let a = argv(OsStr::new("docs/assets/readme/logo-dark.gif"));
        assert_eq!(
            strs(&a),
            [
                "kitten",
                "icat",
                "--hold",
                "docs/assets/readme/logo-dark.gif"
            ]
        );
        assert_eq!(strs(&argv(OsStr::new("A.GIF")))[0], "kitten");
        assert_eq!(strs(&argv(OsStr::new("README.md"))), ["hx", "README.md"]);
        assert_eq!(strs(&argv(OsStr::new("LICENSE"))), ["hx", "LICENSE"]);
        assert_eq!(strs(&argv(OsStr::new("x.gift")))[0], "hx", "the control");
        let spaced = "my docs/a \"b\" $HOME.md";
        let a = argv(OsStr::new(spaced));
        assert_eq!(a.len(), 2);
        assert_eq!(strs(&a), ["hx", spaced]);
        let gif = "two words.gif";
        assert_eq!(at(gif), ["kitten", "icat", "--hold", gif].join("\u{1}"));
        let dash = argv(OsStr::new("-rf.md"));
        assert_eq!(strs(&dash), ["hx", "./-rf.md"]);
        let dash = argv(OsStr::new("--help.gif"));
        assert_eq!(strs(&dash), ["kitten", "icat", "--hold", "./--help.gif"]);
        assert_eq!(
            strs(&argv(OsStr::new("/abs/-x.md"))),
            ["hx", "/abs/-x.md"],
            "an absolute path is left alone"
        );
    }
}
