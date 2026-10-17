//! **baud's version, and the two places outside Cargo that spell it.**
//!
//! The owner, 2026-10-16: "1.0.0-beta.N". The workspace version is the one
//! source: every crate inherits it, `TERM_PROGRAM_VERSION` and XTVERSION
//! read it at compile time (`baud-app/src/pty.rs` holds those two to each
//! other), and `scripts/bundle-macos.sh` writes it into the app bundle's
//! Info.plist, which Apple allows only digits and periods. Beside it the
//! script writes the build number, a counter committed in
//! `packaging/macos/build-number` and raised by hand per release.

use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// `[workspace.package]`'s `version`, read from the manifest rather than
/// from `CARGO_PKG_VERSION`, so a crate that stopped inheriting it shows.
fn workspace_version() -> String {
    let toml = std::fs::read_to_string(workspace().join("Cargo.toml")).expect("Cargo.toml");
    let table = &toml[toml
        .find("[workspace.package]")
        .expect("[workspace.package]")..];
    let line = table
        .lines()
        .find(|l| l.starts_with("version"))
        .expect("[workspace.package] has a version");
    line.split('"').nth(1).expect("a quoted version").to_owned()
}

/// A decimal integer as semver writes one: digits, and no leading zero.
fn integer(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
}

/// The owner's scheme, `1.0.0-beta.N`, with N a positive integer.
#[test]
fn the_version_is_the_owners_scheme() {
    let v = workspace_version();
    let n = v
        .strip_prefix("1.0.0-beta.")
        .unwrap_or_else(|| panic!("{v} is not 1.0.0-beta.N"));
    assert!(integer(n) && n != "0", "{v}: N is a positive integer");
    assert_eq!(env!("CARGO_PKG_VERSION"), v, "baud-app inherits it");
}

/// The `<string>` after `<key>{key}</key>`.
fn plist_string(plist: &str, key: &str) -> String {
    let k = format!("<key>{key}</key>");
    let rest = &plist[plist.find(&k).unwrap_or_else(|| panic!("no {key}")) + k.len()..];
    let open = rest.find("<string>").expect("a <string> follows") + "<string>".len();
    let close = rest.find("</string>").expect("and closes");
    rest[open..close].to_owned()
}

/// **The bundle's two version keys, as the script writes them.**
///
/// `CFBundleShortVersionString`: "The required format is three
/// period-separated integers, such as 10.14.1. The string can only contain
/// numeric characters (0-9) and periods"
/// (`[[source: apple-cfbundleshortversionstring]]`) -- so `1.0.0`, the
/// version less its pre-release part. `CFBundleVersion`: "one to three period-separated
/// integers ... can only contain numeric characters (0-9) and periods"
/// (`[[source: apple-cfbundleversion]]`). The script is run, not read, so
/// what is checked is what it emits.
#[test]
fn the_bundle_says_the_version_as_apple_allows() {
    let out = Command::new("bash")
        .arg(workspace().join("scripts/bundle-macos.sh"))
        .arg("--info-plist")
        .output()
        .expect("bash runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let plist = String::from_utf8(out.stdout).expect("UTF-8");
    assert!(
        !plist.contains("__"),
        "a placeholder left unsubstituted:\n{plist}"
    );

    let v = workspace_version();
    let release = v.split('-').next().unwrap();
    let short = plist_string(&plist, "CFBundleShortVersionString");
    assert_eq!(short, release, "the version less its pre-release part");
    let parts: Vec<&str> = short.split('.').collect();
    assert!(
        parts.len() == 3 && parts.iter().all(|p| integer(p)),
        "{short}"
    );

    let build = plist_string(&plist, "CFBundleVersion");
    let parts: Vec<&str> = build.split('.').collect();
    assert!(
        (1..=3).contains(&parts.len()) && parts.iter().all(|p| integer(p)),
        "CFBundleVersion {build}: one to three integers"
    );
}

/// The build number committed beside the plist: one line, raised by hand
/// for each release.
fn build_number() -> String {
    let path = workspace().join("packaging/macos/build-number");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    text.trim_end_matches('\n').to_owned()
}

/// **`CFBundleVersion` is the committed counter, not anything git knows.**
///
/// The owner, 2026-10-17 (decision 7, the recommended answer): a counter
/// raised by hand per release, kept in the repository, so a build from a
/// tarball or nix, with no `.git`, writes the same number. The script is
/// run, so what is checked is what it emits.
#[test]
fn the_bundles_build_number_is_the_committed_counter() {
    let counter = build_number();
    let parts: Vec<&str> = counter.split('.').collect();
    assert!(
        (1..=3).contains(&parts.len()) && parts.iter().all(|p| integer(p)),
        "packaging/macos/build-number {counter:?}: one to three integers, one line"
    );
    let out = Command::new("bash")
        .arg(workspace().join("scripts/bundle-macos.sh"))
        .arg("--info-plist")
        .output()
        .expect("bash runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let plist = String::from_utf8(out.stdout).expect("UTF-8");
    assert_eq!(
        plist_string(&plist, "CFBundleVersion"),
        counter,
        "the bundle's CFBundleVersion is packaging/macos/build-number"
    );
}
