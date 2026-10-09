//! Shared by the bench binaries: what the host is, asked once.
//!
//! `baud-core` never probes hardware. A bench, like the app, asks the
//! per-arch crate and hands the answer to the storage -- one function, so
//! every binary lays its grid out at the pitch the app would, which is what
//! makes an `ingest2` figure a figure about the shipping layout rather than
//! about a bench's own.

use baud_domain::CacheGeometry;

pub mod flash;

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
