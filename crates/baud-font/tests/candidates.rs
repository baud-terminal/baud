//! **`Face::candidates` never misses a codepoint `Face::glyph` answers.**
//!
//! The candidates are a superset by contract: a caller walks them instead of
//! probing all of Unicode, and asks `glyph` for the answer. Offering too
//! much is harmless; offering too little hides a character the face carries,
//! which the face table would then draw from the wrong face. So this sweeps
//! every codepoint against every face available, covering cmap formats 4,
//! 12 and 14 in the committed faces and the rest in the system's.

use baud_domain::Codepoint;
use baud_font::{Face, FaceIndex};

fn font(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fonts/{name}", env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Every codepoint `glyph` answers lies in a candidate range, and the
/// candidates are not the whole of Unicode -- an enumerator that offered
/// everything would pass the first half trivially.
fn check(name: &str, face: &Face<'_>) -> (u32, u32) {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    face.candidates(|a, b| ranges.push((a.value(), b.value())));
    ranges.sort_unstable();
    // The furthest end among ranges starting at or before each one: `c` is
    // covered exactly when that reaches it.
    let mut reach = Vec::with_capacity(ranges.len());
    let mut far = 0u32;
    for r in &ranges {
        far = far.max(r.1);
        reach.push(far);
    }
    let covered = |c: u32| {
        let i = ranges.partition_point(|r| r.0 <= c);
        i > 0 && reach[i - 1] >= c
    };
    let mut mapped = 0u32;
    for v in 0..=0x10_FFFFu32 {
        let Some(c) = Codepoint::new(v) else { continue };
        if face.glyph(c).is_some() {
            mapped += 1;
            assert!(
                covered(v),
                "{name}: U+{v:04X} is mapped but not a candidate"
            );
        }
    }
    let offered: u32 = ranges.iter().map(|r| r.1 - r.0 + 1).sum();
    assert!(mapped > 0, "{name}: control: the face maps something");
    assert!(
        offered < 0x11_0000,
        "{name}: control: candidates are not all of Unicode"
    );
    (mapped, offered)
}

#[test]
fn every_mapped_codepoint_is_a_candidate_in_the_committed_faces() {
    for name in [
        "JetBrainsMono-Regular.ttf",
        "JetBrainsMono-Variable.ttf",
        "FiraCode-Regular.ttf",
        "NotoColorEmoji-subset.ttf",
        "BungeeTint-Regular.ttf",
    ] {
        let data = font(name);
        let face = Face::parse(&data).expect("parse");
        let (mapped, offered) = check(name, &face);
        eprintln!("{name}: {mapped} mapped, {offered} offered");
    }
}

#[test]
fn every_mapped_codepoint_is_a_candidate_in_the_system_faces() {
    let paths: &[(&str, u32)] = &[
        ("/System/Library/Fonts/Menlo.ttc", 0),
        ("/System/Library/Fonts/Apple Symbols.ttf", 0),
        ("/System/Library/Fonts/Apple Color Emoji.ttc", 0),
        ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
        ("/System/Library/Fonts/AppleSDGothicNeo.ttc", 0),
        ("/System/Library/Fonts/Geneva.ttf", 0),
    ];
    let mut checked = 0;
    for (path, index) in paths {
        let Ok(data) = std::fs::read(path) else {
            continue;
        };
        let Ok(face) = Face::parse_index(&data, FaceIndex::of(*index)) else {
            continue;
        };
        let (mapped, offered) = check(path, &face);
        eprintln!("{path}: {mapped} mapped, {offered} offered");
        checked += 1;
    }
    if cfg!(target_os = "macos") {
        assert!(
            checked >= 5,
            "control: the macOS faces were read ({checked})"
        );
    }
}
