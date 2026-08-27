//! UAX #29 extended grapheme clusters, against Unicode's own conformance suite.
//!
//! `[[source: uax29]]` ships `GraphemeBreakTest.txt`: every case is a codepoint
//! sequence annotated with `÷` at each boundary and `×` where there is none.
//! That is a ready-made suite of several hundred cases written by the people
//! who wrote the rules, which is a far better test than anything invented here
//! — and exactly the kind of "generate valid sequences, do not guess" the
//! property suites already use.
//!
//! Why this exists at all: `docs/research/notes/g-text-encoding.md` deferred
//! grapheme clustering because its cost was **unmeasured**. A cost cannot be
//! measured without an implementation, so this is the implementation, correct
//! against the official suite, so the deferral becomes a decision made on a
//! number rather than on an assumption.

use baud_core::*;

/// One case: the codepoints, and whether a boundary precedes each.
struct Case {
    cps: Vec<u32>,
    breaks: Vec<bool>,
    line: usize,
}

fn cases() -> Vec<Case> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/unicode-graphemebreaktest.txt"
    ))
    .expect("GraphemeBreakTest.txt");
    let mut out = Vec::new();
    for (n, line) in src.lines().enumerate() {
        let body = line.split('#').next().unwrap_or("").trim();
        if body.is_empty() {
            continue;
        }
        let (mut cps, mut breaks) = (Vec::new(), Vec::new());
        let mut pending: Option<bool> = None;
        let mut ok = true;
        for tok in body.split_whitespace() {
            match tok {
                "÷" => pending = Some(true),
                "×" => pending = Some(false),
                hex => match u32::from_str_radix(hex, 16) {
                    Ok(cp) => {
                        cps.push(cp);
                        breaks.push(pending.unwrap_or(true));
                    }
                    Err(_) => ok = false,
                },
            }
        }
        if ok && !cps.is_empty() {
            out.push(Case {
                cps,
                breaks,
                line: n + 1,
            });
        }
    }
    out
}

/// **Every case in Unicode's conformance suite.**
#[test]
fn matches_the_unicode_grapheme_break_suite() {
    let all = cases();
    assert!(
        all.len() > 700,
        "only {} cases parsed -- the suite parser stopped matching",
        all.len()
    );
    let mut failures = Vec::new();
    for c in &all {
        let mut st = ClusterBreak::new();
        let mut got = Vec::with_capacity(c.cps.len());
        for cp in &c.cps {
            let Some(code) = Codepoint::new(*cp) else {
                got.push(true);
                continue;
            };
            got.push(st.is_break(code));
        }
        if got != c.breaks {
            failures.push(format!(
                "line {}: {:04x?}\n      want {:?}\n      got  {:?}",
                c.line, c.cps, c.breaks, got
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} conformance cases failed:\n  {}",
        failures.len(),
        all.len(),
        failures
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// The cases baud actually cares about, named so a regression says what broke.
#[test]
fn the_clusters_a_terminal_meets() {
    let clusters = |s: &str| {
        let mut st = ClusterBreak::new();
        s.chars()
            .filter_map(|c| Codepoint::new(c as u32))
            .filter(|c| st.is_break(*c))
            .count()
    };
    assert_eq!(clusters("a"), 1, "one character");
    assert_eq!(
        clusters("e\u{301}"),
        1,
        "e + combining acute is one cluster"
    );
    assert_eq!(
        clusters("\u{1f469}\u{200d}\u{1f4bb}"),
        1,
        "woman + ZWJ + laptop is one cluster (GB11)"
    );
    assert_eq!(
        clusters("\u{1f1ef}\u{1f1f5}"),
        1,
        "a regional-indicator pair is one flag (GB12/13)"
    );
    assert_eq!(
        clusters("\u{1f1ef}\u{1f1f5}\u{1f1ef}\u{1f1f5}"),
        2,
        "two flags, not one run"
    );
    assert_eq!(
        clusters("\u{915}\u{94d}\u{937}"),
        1,
        "Devanagari conjunct ka + virama + ssa (GB9c)"
    );
    assert_eq!(clusters("\r\n"), 1, "CRLF is one cluster (GB3)");
    assert_eq!(clusters("a\r\nb"), 3, "and does not swallow its neighbours");
}

/// The committed tables match the pinned Unicode data.
#[test]
fn grapheme_tables_match_the_pinned_data() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-grapheme-tables.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "grapheme_data.rs is stale -- rerun scripts/gen-grapheme-tables.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// **A combining mark is attached, not discarded.**
///
/// `put_cp` returned early on `Width::ZERO`, so every combining mark was
/// silently thrown away: `e` + U+0301 stored `e` and the accent was *gone* —
/// not mis-measured, absent, and unrecoverable by a renderer, a selection or a
/// copy. A terminal must not lose a character it was sent.
///
/// The cell keeps the base and the marks live beside the grid, found by
/// position, because `PackedCell` is eight measured bytes with no room.
#[test]
fn where_a_combining_mark_ends_up() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let bytes = "e\u{301}".as_bytes();
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(bytes));
    t.feed(base, f.len());
    t.commit_read(f.len());
    let cols = t.cursor().1.index();
    let cells: Vec<u32> = t.viewport()[..cols.max(1)]
        .iter()
        .map(|c| c.content.as_codepoint().map_or(0, |c| c.value()))
        .collect();
    assert_eq!(cells, vec![0x65], "one cell, holding the base `e`");
    let marks: Vec<u32> = t
        .marks(Row::from_index(0), Col::ZERO)
        .map(|c| c.value())
        .collect();
    assert_eq!(marks, vec![0x301], "and the acute is attached to it");
    assert_eq!(t.marks_dropped.count(), 0, "nothing was lost");
}

/// Several marks stack, and a cell with none reports none.
#[test]
fn marks_stack_and_do_not_leak_between_cells() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    // `a` + acute + diaeresis, then a plain `b`.
    let bytes = "a\u{301}\u{308}b".as_bytes();
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(bytes));
    t.feed(base, f.len());
    t.commit_read(f.len());
    let at = |c: usize| -> Vec<u32> {
        t.marks(Row::from_index(0), Col::at_index(c))
            .map(|x| x.value())
            .collect()
    };
    assert_eq!(at(0), vec![0x301, 0x308], "both marks on the `a`");
    assert_eq!(at(1), Vec::<u32>::new(), "and none on the `b`");
}

/// A mark with no base character is dropped, and counted.
///
/// `[[source: uax29]]` GB1 puts a boundary at start of text, so a leading mark
/// is its own cluster and there is no cell for it to attach to. Dropping it is
/// correct; dropping it *silently* would not be.
#[test]
fn a_leading_mark_has_no_base_and_is_counted() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let bytes = "\u{301}x".as_bytes();
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(bytes));
    t.feed(base, f.len());
    t.commit_read(f.len());
    assert_eq!(t.marks_dropped.count(), 1, "the orphan mark is counted");
}
