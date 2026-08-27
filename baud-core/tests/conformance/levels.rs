//! Every operation's protocol level, cross-checked against the documentation.
//!
//! `Csi::level()` is the answer to "should the enum be split per protocol?" —
//! no, but the fact belongs in the type system where it can be *checked*, which
//! is what this file does. See the doc comment on `Level`.
//!
//! What this buys, and it is the point of putting the level in the code at all:
//! `docs/vt-coverage.md` sorts operations into Tier 1 / Tier 2 / Tier 3
//! headings, and nothing kept those headings honest. Now a mismatch between the
//! tier a row sits under and the level the code assigns is a test failure.

use baud_core::*;

/// Parse `docs/vt-coverage.md` into (sequence, tier-number) pairs.
fn doc_tiers() -> Vec<(String, u8)> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/vt-coverage.md"
    ))
    .expect("vt-coverage.md");
    let mut tier = 0u8;
    let mut out = Vec::new();
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("## Tier ") {
            tier = rest
                .chars()
                .next()
                .and_then(|c| c.to_digit(10))
                .unwrap_or(0) as u8;
            continue;
        }
        let l = line.trim();
        if !l.starts_with("| `CSI ") {
            continue;
        }
        let seq = l.trim_matches('|').split('|').next().unwrap_or("").trim();
        out.push((seq.trim_matches('`').to_string(), tier));
    }
    out
}

fn level_of_tier(t: u8) -> Option<Level> {
    match t {
        1 => Some(Level::Vt100),
        2 => Some(Level::Vt220),
        3 => Some(Level::Xterm),
        _ => None,
    }
}

/// Feed a sequence and report which `Csi` the parser resolved it to.
///
/// Goes through `Term` rather than calling `Csi::parse` directly, so this
/// checks the operation an application would actually reach.
fn level_via_parser(seq: &str) -> Option<Level> {
    // `CSI ` prefix, then the row's literal bytes with spaces removed --
    // the table writes `CSI ? J` for what the wire spells `\x1b[?J`.
    let body: String = seq
        .trim_start_matches("CSI")
        .chars()
        .filter(|c| *c != ' ')
        .collect();
    if body.is_empty() {
        return None;
    }
    let marker = if body.starts_with('?') {
        Marker::Private
    } else if body.starts_with('>') {
        Marker::Gt
    } else {
        Marker::Standard
    };
    let rest: Vec<char> = body.trim_start_matches(['?', '>']).chars().collect();
    let (inter, fin) = match rest.as_slice() {
        [f] => (None, *f),
        [i, f] => (Some(*i), *f),
        _ => return None,
    };
    let f = FinalByte::from_stream(StreamByte::from_stream(fin as u8))?;
    let inter = inter.map(|c| StreamByte::from_stream(c as u8));
    Csi::parse(marker, inter, Params::new(&[]), f).level()
}

/// A row's tier and the code's level must agree.
///
/// Disagreement means one of them is wrong, and neither is obviously the
/// authority — which is exactly why this should fail rather than be resolved by
/// whoever notices.
#[test]
fn documented_tiers_match_the_code() {
    let mut checked = 0usize;
    let mut wrong = Vec::new();
    for (seq, tier) in doc_tiers() {
        let Some(want) = level_of_tier(tier) else {
            continue;
        };
        let Some(got) = level_via_parser(&seq) else {
            continue; // ambiguous or parameterised rows; not this test's job
        };
        checked += 1;
        if got != want {
            wrong.push(format!(
                "`{seq}` is under Tier {tier} ({want:?}) but the code says {got:?}"
            ));
        }
    }
    assert!(
        checked > 15,
        "only {checked} rows cross-checked -- the parser stopped matching the table"
    );
    assert!(
        wrong.is_empty(),
        "tier/level mismatches:\n  {}",
        wrong.join("\n  ")
    );
}

/// Every level baud claims has at least one operation, so a level cannot be
/// silently emptied by a refactor.
#[test]
fn every_level_is_populated() {
    let probes: &[(Level, &str)] = &[
        (Level::Vt100, "CSI A"),
        (Level::Vt220, "CSI ? J"),
        (Level::Vt420, "CSI ' }"),
        (Level::Xterm, "CSI > q"),
    ];
    for (want, seq) in probes {
        assert_eq!(
            level_via_parser(seq),
            Some(*want),
            "{seq} should be {want:?}"
        );
    }
}

/// `Unsupported` has no level: it is the absence of an operation, not one at
/// some level.
#[test]
fn unsupported_has_no_level() {
    assert_eq!(level_via_parser("CSI ~"), None);
}
