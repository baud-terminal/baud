//! **The door `main` opens first, before anything else runs.**
//!
//! - A probe build (the test-only `effect-probe` feature) is only ever a
//!   rig. Its arguments must be a well-formed [`RigCall`], or it exits 2,
//!   before any application, window, activation policy or Dock work, so
//!   no probe binary can become a real app. Off macOS there is no rig,
//!   and a probe build always exits 2.
//! - A build without the probe takes no rig's flag for a plain launch:
//!   any argument spelled as one (`--…-rig`) exits 2.
//!
//! A rig's flags are matched in one place, `baud-present-macos`'s
//! `rig_call.rs`; the source check here fails on any other.
//!
//! [`RigCall`]: baud_present_macos::rig_call::RigCall

/// What `main` may go on to run.
pub(crate) enum Admitted {
    /// The app: never in a probe build.
    #[cfg_attr(
        feature = "effect-probe",
        allow(dead_code, reason = "a probe build never runs the app")
    )]
    App,
    /// A rig, as called.
    #[cfg(all(target_os = "macos", feature = "effect-probe"))]
    Rig(baud_present_macos::rig_call::RigCall),
}

/// The refusal, or what `main` may run.
pub(crate) fn admitted() -> Admitted {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(all(target_os = "macos", feature = "effect-probe"))]
    match baud_present_macos::rig_call::RigCall::parse(&args) {
        Ok(call) => Admitted::Rig(call),
        Err(why) => refuse(why.0),
    }
    #[cfg(all(not(target_os = "macos"), feature = "effect-probe"))]
    {
        let _ = args;
        refuse("a probe build runs only as macOS's rig")
    }
    #[cfg(not(feature = "effect-probe"))]
    match args.iter().any(|a| names_a_rig(a)) {
        true => refuse("this build has no rig"),
        false => Admitted::App,
    }
}

/// Said, then exit 2.
fn refuse(why: &str) -> ! {
    eprintln!("baud: {why}");
    std::process::exit(2)
}

/// Whether `arg` is spelled as a rig's flag, `--` and a name ending in
/// `-rig`: every mode's, without naming one.
#[cfg_attr(feature = "effect-probe", allow(dead_code))]
fn names_a_rig(arg: &str) -> bool {
    arg.strip_prefix("--")
        .and_then(|a| a.strip_suffix("-rig"))
        .is_some_and(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A rig's flag never reaches a plain launch**, whatever mode it
    /// names. The control: a plain argument and a flag that only ends in
    /// `rig`. Perturbation: match `--event-rig` alone, and the second name
    /// passes.
    #[test]
    fn a_rigs_flag_is_never_a_plain_launch() {
        let flag = |name: &str| format!("--{name}-rig");
        assert!(names_a_rig(&flag("event")));
        assert!(names_a_rig(&flag("hover")));
        assert!(!names_a_rig("README.md"));
        assert!(!names_a_rig("--rig"));
        assert!(!names_a_rig("--configrig"));
    }

    /// **The door is `main`'s first statement, and a rig's flag is spelled
    /// in one file**: every `.rs` under baud-app and baud-present-macos,
    /// and the scripts, read for a `--…-rig` string literal; only
    /// `rig_call.rs` may hold one. The control is that file's own. The
    /// perturbations: `main` reading `version::asks` before the door, or a
    /// script that spells the flag.
    #[test]
    fn the_door_is_first_and_the_flag_is_spelled_once() {
        let main = include_str!("main.rs");
        let body = main
            .split_once("\nfn main() {\n")
            .map(|(_, b)| b)
            .expect("main");
        let first = body
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with("//"))
            .expect("a statement");
        assert_eq!(first, "let admitted = rig_door::admitted();");

        /// Which kind of source a file is.
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Kind {
            Rust,
            Script,
        }
        // `--` and a name of letters and hyphens ending in `-rig`: in Rust,
        // a string literal's start; in a script, any word.
        fn spells(text: &str, kind: Kind) -> bool {
            text.match_indices("--").any(|(at, _)| {
                let before = text[..at].chars().next_back();
                let name: String = text[at + 2..]
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                    .collect();
                name.len() > 4
                    && name.ends_with("-rig")
                    && (kind == Kind::Script || before == Some('"'))
            })
        }
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let kind = match path.extension().and_then(|e| e.to_str()) {
                    Some("rs") => Some(Kind::Rust),
                    Some("sh" | "nix") => Some(Kind::Script),
                    _ => None,
                };
                if path.is_dir() {
                    walk(&path, out);
                } else if kind
                    .is_some_and(|k| std::fs::read_to_string(&path).is_ok_and(|s| spells(&s, k)))
                {
                    out.push(path);
                }
            }
        }
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the workspace");
        let mut found = Vec::new();
        for dir in [
            "baud-app/src",
            "baud-app/tests",
            "baud-present-macos/src",
            "scripts",
        ] {
            walk(&repo.join(dir), &mut found);
        }
        let names: Vec<String> = found
            .iter()
            .map(|p| p.strip_prefix(repo).unwrap_or(p).display().to_string())
            .collect();
        assert_eq!(
            names,
            ["baud-present-macos/src/rig_call.rs"],
            "a rig's flag spelled outside its one parser: {found:?}"
        );
    }
}
