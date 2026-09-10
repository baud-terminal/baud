//! VT100 / ANSI — the set every terminal must implement.
//!
//! One module per area rather than one flat file per test, so the protocol a
//! test belongs to is visible from its path: `tests/vt100/scrolling.rs` is
//! VT100 scrolling, and nothing else is.
//!
//! Conformance against the `vt100`, `vt102` and `ansi` terminfo profiles lives
//! in `tests/conformance/` — these are the behavioural tests behind it.

#[path = "../support/mod.rs"]
mod support;

mod c0;
mod charsets;
mod cursor;
mod erase;
mod irm;
mod modes;
mod private_modes;
mod scrolling;
mod tabs;

/// **The VT100 gaps `baud-core` still has, pinned as a list.**
///
/// Moving `baud-core2`'s suite across found six capability gaps and two
/// defects; five are closed and three are not, and an `#[ignore]` with a
/// reason is how a known gap stays visible without a red build. On its own
/// that is the probe-that-cannot-fire this repo records seven times -- an
/// ignore nobody counts is a test nobody runs -- so the list is asserted.
///
/// A third ignore fails this. Fixing one of the two fails it too, which is
/// the point: the pin has to move when the gap does, in either direction.
/// Same instrument as `the_glyph_gap_is_pinned`, which pins non-zero gaps at
/// what they *are* rather than asserting zero.
///
/// What each one is:
///
/// * **DECAWM off leaves the cursor one past the last column**, where it
///   should stay on it. `ICH` then acts off the row.
/// * **`?45` reverse wraparound is not recorded**, so `DECRQM` answers `0`.
///   Recording it without acting on it would be the worse half, so this is a
///   feature rather than an accessor.
#[test]
fn the_known_vt100_gaps_are_these_two() {
    let mut ignored = Vec::new();
    for f in [
        "c0.rs",
        "charsets.rs",
        "cursor.rs",
        "erase.rs",
        "irm.rs",
        "modes.rs",
        "private_modes.rs",
        "scrolling.rs",
        "tabs.rs",
    ] {
        let p = format!("{}/tests/vt100/{f}", env!("CARGO_MANIFEST_DIR"));
        let src = std::fs::read_to_string(&p).expect("suite file");
        for (i, line) in src.lines().enumerate() {
            if line.trim_start().starts_with("#[ignore") {
                let name = src
                    .lines()
                    .skip(i)
                    .find_map(|l| {
                        l.trim_start()
                            .strip_prefix("fn ")
                            .map(|n| n.split('(').next().unwrap_or(n).to_string())
                    })
                    .unwrap_or_else(|| "<no fn follows>".into());
                ignored.push(name);
            }
        }
    }
    ignored.sort();
    assert_eq!(
        ignored,
        [
            "autowrap_off_leaves_the_cursor_on_the_last_column",
            "decrqm_reports_each_mode",
        ],
        "the ignore list moved -- a gap was added or closed, and this pin has \
         to move with it"
    );
}
