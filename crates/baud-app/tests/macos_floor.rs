//! **One floor, macOS 26, set in four places.** baud runs on macOS 26
//! and later. The release is written where each layer reads it, and
//! nothing ties the places together but this file:
//!
//! * the bundle's `LSMinimumSystemVersion` (`nix/packaging/macos/Info.plist`),
//!   which Launch Services checks before the app runs;
//! * the deployment target (`.cargo/config.toml`), which rustc and the
//!   linker write into the binary as `LC_BUILD_VERSION minos`;
//! * the same variable from `crates/baud-app/build.rs`, for the published crate,
//!   which carries no `.cargo/config.toml`: `cargo install baud` would
//!   otherwise link for rustc's default, macOS 11;
//! * the Metal compiler's `-mmacosx-version-min` (`tools/scripts/build-metallib.sh`).
//!
//! The committed library's own target triples, `air64_v..-apple-macosx..`,
//! say what the script actually produced.
//!
//! The first test reads files and runs everywhere; the second reads this
//! test binary's own load commands, so it runs on macOS only, and proves
//! the config reached the link rather than that the config says 26.

use std::path::PathBuf;

/// The release, as `LSMinimumSystemVersion` spells it.
const FLOOR: &str = "26.0";

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(workspace().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The `<string>` after `<key>LSMinimumSystemVersion</key>`.
fn plist_floor() -> String {
    let plist = read("nix/packaging/macos/Info.plist");
    let key = "<key>LSMinimumSystemVersion</key>";
    let at = plist
        .find(key)
        .expect("Info.plist names LSMinimumSystemVersion")
        + key.len();
    let rest = &plist[at..];
    let open = rest.find("<string>").expect("a <string> follows the key") + "<string>".len();
    let close = rest[open..].find("</string>").expect("the <string> closes") + open;
    rest[open..close].trim().to_owned()
}

/// `MACOSX_DEPLOYMENT_TARGET`'s value in `.cargo/config.toml`, and whether
/// the entry is forced over the environment's.
fn config_floor() -> (String, bool) {
    let config = read(".cargo/config.toml");
    let line = config
        .lines()
        .find(|l| l.trim_start().starts_with("MACOSX_DEPLOYMENT_TARGET"))
        .expect(".cargo/config.toml sets MACOSX_DEPLOYMENT_TARGET");
    let value = line.split('"').nth(1).expect("a quoted value").to_owned();
    (value, line.contains("force = true"))
}

/// Every `-apple-macosxX.Y.Z` triple in the committed library, as `X.Y.Z`.
fn metallib_floors() -> Vec<String> {
    let lib = std::fs::read(workspace().join("crates/baud-present-macos/src/baud.metallib"))
        .expect("the committed library");
    let needle = b"-apple-macosx";
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = lib[i..].windows(needle.len()).position(|w| w == needle) {
        let start = i + off + needle.len();
        let end = lib[start..]
            .iter()
            .position(|b| !(b.is_ascii_digit() || *b == b'.'))
            .map_or(lib.len(), |n| start + n);
        out.push(String::from_utf8_lossy(&lib[start..end]).into_owned());
        i = end;
    }
    out
}

#[test]
fn the_bundle_the_binary_and_the_shaders_name_one_floor() {
    assert_eq!(plist_floor(), FLOOR, "Info.plist's LSMinimumSystemVersion");

    let (target, forced) = config_floor();
    assert_eq!(
        target, FLOOR,
        ".cargo/config.toml's MACOSX_DEPLOYMENT_TARGET"
    );
    // devenv exports 14.0, and an unforced [env] entry yields to it.
    assert!(
        forced,
        "MACOSX_DEPLOYMENT_TARGET must be forced over devenv's"
    );

    let env = format!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET={FLOOR}\")");
    assert!(
        read("crates/baud-app/build.rs").contains(&env),
        "crates/baud-app/build.rs must print {env}"
    );

    let script = read("tools/scripts/build-metallib.sh");
    let flag = format!("-mmacosx-version-min={FLOOR} ");
    assert!(
        script.contains(&flag),
        "tools/scripts/build-metallib.sh must pass {flag}"
    );

    // The positive control is the count: a library with no triple at all
    // would pass an "every triple" check vacuously.
    let triples = metallib_floors();
    assert!(
        !triples.is_empty(),
        "no target triple found in baud.metallib"
    );
    let want = format!("{FLOOR}.0");
    assert!(
        triples.iter().all(|t| *t == want),
        "baud.metallib targets {triples:?}, want every one {want}"
    );
}

/// **The installed app and the dev shell link an SDK of 26 or later**, as
/// `cargo install` links Xcode's: AppKit lays the title bar out by the
/// linked SDK, so an app linked against nixpkgs' default, 14.4, would draw
/// the bar from before macOS 26 beside a clone's build that draws the new
/// one. The flake's macOS build and devenv's `apple.sdk` both take
/// `nix/darwin-sdk.nix`, and every SDK it names is 26 or later;
/// `checks.<darwin>.sdk` reads the built app's `LC_BUILD_VERSION` back, and
/// [`this_binary_was_linked_against_an_sdk_of_26_or_later`] this test's.
/// Named break: leave `buildInputs` or `apple.sdk` out, or name
/// `apple-sdk_15`.
#[test]
fn the_installed_app_links_an_sdk_of_26_or_later() {
    let flake = read("flake.nix");
    assert!(
        flake.contains("darwinSdk = import ./nix/darwin-sdk.nix;"),
        "the flake's darwinSdk is nix/darwin-sdk.nix"
    );
    assert!(
        flake.contains(
            "buildInputs = lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ (darwinSdk pkgs) ];"
        ),
        "the flake's macOS build takes darwinSdk"
    );
    assert!(
        read("devenv.nix")
            .contains("apple.sdk = if isDarwin then import ./nix/darwin-sdk.nix pkgs else null;"),
        "the dev shell's apple.sdk is nix/darwin-sdk.nix"
    );
    let file = read("nix/darwin-sdk.nix");
    let body = &file[file.find("pkgs:").expect("the file is a function of pkgs")..];
    let sdks: Vec<u32> = body
        .match_indices("apple-sdk_")
        .map(|(at, m)| {
            let digits: String = body[at + m.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse().expect("an SDK's major release")
        })
        .collect();
    // The control: a definition naming no SDK would pass "every one" vacuously.
    assert!(
        !sdks.is_empty(),
        "nix/darwin-sdk.nix names an apple-sdk: {body}"
    );
    assert!(
        sdks.iter().all(|&major| major >= 26),
        "nix/darwin-sdk.nix names {sdks:?}, every one 26 or later"
    );
}

/// `LC_BUILD_VERSION`'s `minos` and `sdk`, each packed `xxxx.yy.zz` in
/// nibbles, from a thin 64-bit Mach-O.
#[cfg(target_os = "macos")]
fn build_version(bin: &[u8]) -> ((u32, u32, u32), (u32, u32, u32)) {
    const MH_MAGIC_64: u32 = 0xfeed_facf;
    const LC_BUILD_VERSION: u32 = 0x32;
    let word = |at: usize| u32::from_le_bytes(bin[at..at + 4].try_into().expect("four bytes"));
    assert_eq!(word(0), MH_MAGIC_64, "a thin 64-bit Mach-O");
    let ncmds = word(16) as usize;
    // mach_header_64 is 32 bytes; each load command starts {cmd, cmdsize}.
    let mut at = 32;
    for _ in 0..ncmds {
        let (cmd, size) = (word(at), word(at + 4) as usize);
        if cmd == LC_BUILD_VERSION {
            // build_version_command: cmd, cmdsize, platform, minos, sdk, ...
            let unpack = |v: u32| (v >> 16, (v >> 8) & 0xff, v & 0xff);
            return (unpack(word(at + 12)), unpack(word(at + 16)));
        }
        at += size;
    }
    panic!("no LC_BUILD_VERSION among {ncmds} load commands");
}

#[cfg(target_os = "macos")]
#[test]
fn this_binary_was_linked_for_the_floor() {
    let exe = std::env::current_exe().expect("this test's path");
    let bin = std::fs::read(&exe).expect("this test's bytes");
    let ((major, minor, patch), _) = build_version(&bin);
    assert_eq!(
        format!("{major}.{minor}"),
        FLOOR,
        "{} links at minos {major}.{minor}.{patch}",
        exe.display()
    );
}

/// **This binary was linked against an SDK of 26 or later**, as the
/// installed app and a `cargo install` are: the dev shell's SDKROOT is the
/// flake's `nix/darwin-sdk.nix`, and AppKit lays the title bar out by the
/// linked SDK, so a build under nixpkgs' default 14.4 draws the bar from
/// before macOS 26 and the window tests measure a layout no release has.
/// Red in a shell whose SDKROOT is 14.4.
#[cfg(target_os = "macos")]
#[test]
fn this_binary_was_linked_against_an_sdk_of_26_or_later() {
    let exe = std::env::current_exe().expect("this test's path");
    let bin = std::fs::read(&exe).expect("this test's bytes");
    let (_, (major, minor, patch)) = build_version(&bin);
    assert!(
        major >= 26,
        "{} was linked against SDK {major}.{minor}.{patch}",
        exe.display()
    );
}
