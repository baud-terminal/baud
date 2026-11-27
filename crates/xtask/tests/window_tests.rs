//! **The window tests run only when they are named.** `titlebar_mark` and
//! `about_window` build AppKit windows, and no run that does not name them
//! runs them: the test runner's default filter leaves them out of every
//! `cargo nextest run`, so the scoped push gate, the tries and the full suite
//! skip them, and `--ignore-default-filter` or `cargo test --test <name>`
//! runs them.

use std::fs;

#[test]
fn the_window_tests_are_left_out_of_every_unnamed_run() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.config/nextest.toml");
    let text = fs::read_to_string(path).expect("the test runner's configuration");
    let table: toml::Table = text.parse().expect("it parses");
    let filter = table
        .get("profile")
        .and_then(|p| p.get("default"))
        .and_then(|d| d.get("default-filter"))
        .and_then(|f| f.as_str())
        .expect("the default profile has a default filter");
    for name in ["titlebar_mark", "about_window"] {
        assert!(
            filter.contains(&format!("binary({name})")) && filter.trim_start().starts_with("not "),
            "{name} is not left out by the default filter: {filter}"
        );
    }
}
