//! Shared by the bench binaries: what the host is, asked once.
//!
//! `baud-core` never probes hardware. A bench, like the app, asks the
//! per-arch crate and hands the answer to the storage -- one function, so
//! every binary lays its grid out at the pitch the app would, which is what
//! makes an `ingest2` figure a figure about the shipping layout rather than
//! about a bench's own.

use baud_core::RunScan;
use baud_domain::CacheGeometry;

pub mod flash;

/// The scan this crate's own features name, by the three `cfg`s that pick
/// `baud_core::RUN_SCAN`, read over `neon-scan` and `avx2-scan` here.
/// `ingest2` will not build when the two differ: a feature another crate
/// in the build turned on in `baud-core` would make the arm measure a
/// scan it is not named for.
#[cfg(all(feature = "neon-scan", target_arch = "aarch64"))]
pub const SCAN_ARM: RunScan = RunScan::Neon;
/// The scan this crate's own features name.
#[cfg(all(feature = "avx2-scan", target_arch = "x86_64", target_feature = "avx2"))]
pub const SCAN_ARM: RunScan = RunScan::Avx2;
/// The scan this crate's own features name.
#[cfg(not(any(
    all(feature = "neon-scan", target_arch = "aarch64"),
    all(feature = "avx2-scan", target_arch = "x86_64", target_feature = "avx2")
)))]
pub const SCAN_ARM: RunScan = RunScan::Portable;

/// The cache hierarchy of this machine, or `UNKNOWN` where the probe cannot
/// ask -- which still resolves to a safe pitch.
pub fn host_geometry() -> CacheGeometry {
    #[cfg(target_arch = "aarch64")]
    {
        <baud_isa_aarch64::Host as baud_isa::CacheProbe>::detect()
    }
    #[cfg(target_arch = "x86_64")]
    {
        <baud_isa_x86_64::Host as baud_isa::CacheProbe>::detect()
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        CacheGeometry::UNKNOWN
    }
}

/// **The faces the Linux driver reads with the default config**, in the
/// order `config.rs`'s Linux `default_faces` sets: the embedded Regular and
/// icon face, then `system` -- the system faces the default stack finds,
/// where a bench is told of any -- then STIX Two Math, the embedded Bold,
/// Italic and Bold Italic as the styled faces and the Bold as the badge's.
/// Each is a file's bytes, the face in it and its role.
pub fn linux_faces(
    system: Vec<(Vec<u8>, baud_font::FaceIndex)>,
) -> Vec<(Vec<u8>, baud_font::FaceIndex, baud_render::FaceRole)> {
    use baud_render::{FaceRole, FaceStyle};
    let fonts = format!("{}/../baud-app/fonts", env!("CARGO_MANIFEST_DIR"));
    let read = |rel: &str| std::fs::read(format!("{fonts}/{rel}")).expect("an embedded face");
    let first = baud_font::FaceIndex::FIRST;
    let mut files = vec![
        (read("built/Regular.ttf"), first, FaceRole::Fallback),
        (
            read("symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
            first,
            FaceRole::Fallback,
        ),
    ];
    files.extend(
        system
            .into_iter()
            .map(|(bytes, index)| (bytes, index, FaceRole::Fallback)),
    );
    files.extend([
        (
            read("stix-two-math/STIXTwoMath-Regular.otf"),
            first,
            FaceRole::Fallback,
        ),
        (
            read("built/Bold.ttf"),
            first,
            FaceRole::Style(FaceStyle::Bold),
        ),
        (
            read("built/Italic.ttf"),
            first,
            FaceRole::Style(FaceStyle::Italic),
        ),
        (
            read("built/BoldItalic.ttf"),
            first,
            FaceRole::Style(FaceStyle::BoldItalic),
        ),
        (read("built/Bold.ttf"), first, FaceRole::Badge),
    ]);
    files
}

/// The positional arguments a bench was given, with `cargo bench`'s own
/// trailing flag removed.
///
/// `cargo bench --bench ingest2 -- less 400` runs the target with
/// `[exe, "less", "400", "--bench"]`: cargo appends `--bench` after the
/// caller's arguments, for the libtest harness these targets do not use.
/// Measured, not assumed -- and it matters, because every bench here reads
/// positionals in order, so the flag would arrive as one. `frame` and
/// `frame_gpu` read a third (the damage mode) and `glyphs` reads a third
/// (the probe), so a run that omitted it would have been handed `--bench`
/// instead of nothing.
///
/// Only a *trailing* `--bench` is dropped, so an argument that happens to
/// spell it cannot be eaten from the middle.
pub fn args() -> impl Iterator<Item = String> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    without_bench_flag(a).into_iter()
}

/// The rule itself, over the arguments rather than the process, so it has a
/// test: drop one trailing `--bench` and nothing else.
fn without_bench_flag(mut a: Vec<String>) -> Vec<String> {
    if a.last().map(String::as_str) == Some("--bench") {
        a.pop();
    }
    a
}

#[cfg(test)]
mod tests {
    use super::without_bench_flag;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_trailing_bench_flag_is_dropped_and_nothing_else_is() {
        // What `cargo bench --bench ingest2 -- less 400` actually delivers,
        // measured rather than assumed.
        assert_eq!(
            without_bench_flag(v(&["less", "400", "--bench"])),
            v(&["less", "400"])
        );
        // And with no arguments of the caller's at all, which is the case
        // that would otherwise hand `frame` a damage mode of "--bench".
        assert_eq!(without_bench_flag(v(&["--bench"])), Vec::<String>::new());
        // A positive control: the rule must not fire when cargo did not add
        // it, or a bench run as a plain binary would lose its last argument.
        assert_eq!(without_bench_flag(v(&["less", "400"])), v(&["less", "400"]));
        assert_eq!(without_bench_flag(v(&[])), Vec::<String>::new());
        // Only the tail, so an argument that spells it is not eaten from the
        // middle.
        assert_eq!(
            without_bench_flag(v(&["--bench", "400"])),
            v(&["--bench", "400"])
        );
    }
}
