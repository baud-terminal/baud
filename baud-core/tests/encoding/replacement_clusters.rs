//! **A U+FFFD written for ill-formed bytes is a character to the cluster
//! rules exactly as a literal `EF BF BD` is.** The owner's decision of
//! 2026-10-13, SAME_AS_LITERAL, over AS_BUILT (the substitute always breaks
//! before it) and DECLARED_PROFILE (a UAX29-C1-2 profile saying so): the
//! record, "A U+FFFD for ill-formed bytes clusters as a literal one".
//!
//! `[[source: unicode-ch3]]` 3.9.5 has a converter substitute "one or more
//! U+FFFD replacement characters", and `[[source: uax29]]` segments a
//! sequence of characters and says nothing of how one was produced, so after
//! the substitution the U+FFFD is the character U+FFFD, class Other
//! (`[[source: gbp-data]]`'s `@missing` line), and GB9b joins it to a Prepend
//! before it as it joins the literal one.

use baud_core::*;

/// Four rows of twenty: every case fits on the first.
const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

/// Everything a cell shows of a cluster, for every cell of the screen, the
/// cursor, the marks dropped and the replacement count, after `bytes` fed as
/// `cuts` says under ambiguous width `amb`. A cell is `_` blank, `tail` for a
/// wide character's right half, or its codepoint and every mark stored with
/// it, so a U+FFFD that attaches and one that takes a cell cannot read alike.
type Seen = (Vec<String>, (Row, Col), usize, usize);

fn seen(bytes: &[u8], cuts: &[usize], amb: AmbiguousWidth) -> Seen {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    t.set_ambiguous_width(amb);
    let mut at = 0;
    for &end in cuts.iter().chain([bytes.len()].iter()) {
        let chunk = &bytes[at..end];
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        at = end;
    }
    let mut cells = Vec::new();
    for y in 0..usize::from(WS.ws_row) {
        let y = Row::from_index(y);
        for (x, c) in t.row(y).expect("a row on the grid").iter().enumerate() {
            cells.push(match (c.content.is_empty(), c.content.is_wide_tail()) {
                (true, _) => "_".to_string(),
                (_, true) => "tail".to_string(),
                _ => {
                    let base = t
                        .base_of(*c)
                        .as_codepoint()
                        .expect("a written cell holds one");
                    let mut s = format!("{:X}", base.value());
                    for m in t.marks(y, Col::at_index(x)) {
                        s.push_str(&format!("+{m:X}"));
                    }
                    s
                }
            });
        }
    }
    (cells, t.cursor(), t.marks_dropped, t.replacements)
}

/// Named ways of cutting a case into reads: where each read ends.
type Cuts = Vec<(String, Vec<usize>)>;

/// Every read size from one byte up, then every split into two reads.
fn cuts(n: usize) -> (Cuts, Cuts) {
    let sizes = (1..=n)
        .map(|size| {
            (
                format!("reads of {size}"),
                (size..n).step_by(size).collect(),
            )
        })
        .collect();
    let splits = (1..n)
        .map(|at| (format!("split at byte {at}"), vec![at]))
        .collect();
    (sizes, splits)
}

fn utf8(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// What comes before the U+FFFD: one representative of every
/// `Grapheme_Cluster_Break` value, and of `Extended_Pictographic` and the
/// `InCB` consonant and linker, a zero-width one after an `a` so it has a
/// base. The four that end in a Prepend after a letter are where AS_BUILT and
/// SAME_AS_LITERAL differ (GB9b).
fn befores() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("Other a", utf8("a")),
        ("CR (after a)", utf8("a\r")),
        ("LF (after a)", utf8("a\n")),
        ("Control U+0007 (after a)", utf8("a\u{7}")),
        ("Control U+200B ZWSP (after a)", utf8("a\u{200b}")),
        ("Extend U+0301 (after a)", utf8("a\u{301}")),
        ("ZWJ (after a)", utf8("a\u{200d}")),
        ("ZWJ after ExtPict U+1F468", utf8("\u{1f468}\u{200d}")),
        ("Regional_Indicator U+1F1E6", utf8("\u{1f1e6}")),
        ("Prepend U+0600 (after a)", utf8("a\u{600}")),
        ("Prepend U+0600 at start", utf8("\u{600}")),
        ("Prepend U+06DD (after a)", utf8("a\u{6dd}")),
        ("Prepend U+0D4E (after a)", utf8("a\u{d4e}")),
        ("Prepend U+0600 U+0600 (after a)", utf8("a\u{600}\u{600}")),
        ("SpacingMark U+0903 (after a)", utf8("a\u{903}")),
        ("L U+1100", utf8("\u{1100}")),
        ("V U+1161 (after a)", utf8("a\u{1161}")),
        ("T U+11A8 (after a)", utf8("a\u{11a8}")),
        ("LV U+AC00", utf8("\u{ac00}")),
        ("LVT U+AC01", utf8("\u{ac01}")),
        ("ExtPict U+1F468", utf8("\u{1f468}")),
        ("InCB Consonant U+0915", utf8("\u{915}")),
        (
            "InCB Consonant+Linker U+0915 U+094D",
            utf8("\u{915}\u{94d}"),
        ),
        ("Other U+FFFD literal", utf8("\u{fffd}")),
        ("Other U+00E9", utf8("\u{e9}")),
    ]
}

/// What comes after it.
fn afters() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("Z", utf8("Z")),
        ("U+0301", utf8("\u{301}")),
        ("U+0903", utf8("\u{903}")),
        ("U+1F1E7", utf8("\u{1f1e7}")),
        ("ZWJ U+1F469", utf8("\u{200d}\u{1f469}")),
        ("U+0915", utf8("\u{915}")),
        ("U+1161", utf8("\u{1161}")),
    ]
}

/// The ill-formed bytes, each one maximal subpart and so one U+FFFD, and
/// each reaching one of the three places a substitute is written: a stray
/// continuation and two bytes no sequence begins with (`Act::Bad`), and a
/// lead cut short, which a read that ends inside it hands to `Act::Resync`
/// and a read that holds it whole to `chars`.
fn bads() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("80", vec![0x80]),
        ("C0", vec![0xc0]),
        ("FF", vec![0xff]),
        ("E2 82", vec![0xe2, 0x82]),
        ("F0 9F", vec![0xf0, 0x9f]),
    ]
}

const LITERAL: [u8; 3] = [0xef, 0xbf, 0xbd];

/// **`X <bad> Y` is `X EF BF BD Y`, screen, cursor and marks, for every
/// `X`, `Y` and spelling of the bad bytes, at every read size and every split
/// into two reads, in the ANSI and the VT52 ground, under both ambiguous
/// widths.** The pair space is the decision's prototype's: 25 befores, 7
/// afters, 5 spellings and every read size, 7,075 pairs a ground and width.
///
/// The bad side's replacement count is one, a U+FFFD that attaches included,
/// and the literal side's is none: `Term::replacements` counts the U+FFFDs
/// written for bytes that could not be decoded, and a literal U+FFFD was
/// decoded.
///
/// U+FFFD is East Asian Width A (`[[source: uax11]]`), so under
/// `AmbiguousWidth::Wide` the substitute is two columns as the literal is.
#[test]
fn a_replacement_for_ill_formed_bytes_clusters_as_a_literal_one() {
    let grounds: [(&str, &[u8]); 2] = [("ANSI", b""), ("VT52", b"\x1b[?2l")];
    let mut sized = 0usize;
    let mut split = 0usize;
    let mut differ = Vec::new();
    let mut counts = Vec::new();
    // How many differ in each ground and width, at read sizes and at splits.
    let mut tally: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    let mut prepend_pairs = 0usize;
    for amb in [AmbiguousWidth::Narrow, AmbiguousWidth::Wide] {
        for (gname, enter) in grounds {
            let mut sized_here = 0usize;
            for (xn, x) in befores() {
                for (yn, y) in afters() {
                    for (bn, b) in bads() {
                        let bad = [enter, &x, &b, &y].concat();
                        let lit = [enter, &x, &LITERAL[..], &y].concat();
                        let want = seen(&lit, &[], amb);
                        if want.3 != 0 {
                            counts.push(format!(
                                "{gname} {amb:?}: {xn} | EF BF BD | {yn}: {} replacements for a literal",
                                want.3
                            ));
                        }
                        // The cuts fall in the case, not in the mode switch.
                        let (sizes, splits) = cuts(bad.len() - enter.len());
                        let shift = |at: &[usize]| -> Vec<usize> {
                            core::iter::once(enter.len())
                                .filter(|&e| e > 0)
                                .chain(at.iter().map(|a| a + enter.len()))
                                .collect()
                        };
                        for (k, (cut, at)) in sizes.iter().chain(splits.iter()).enumerate() {
                            match k < sizes.len() {
                                true => sized_here += 1,
                                false => split += 1,
                            }
                            prepend_pairs +=
                                usize::from(xn.starts_with("Prepend") && xn.contains("after a"));
                            let got = seen(&bad, &shift(at), amb);
                            let what = format!("{gname} {amb:?}: {xn} | {bn} | {yn}, {cut}");
                            if (&got.0, got.1, got.2) != (&want.0, want.1, want.2) {
                                let t = tally.entry(format!("{gname} {amb:?}")).or_default();
                                match k < sizes.len() {
                                    true => t.0 += 1,
                                    false => t.1 += 1,
                                }
                                differ.push(format!(
                                    "{what}: {:?} {:?} for {:?} {:?}",
                                    &got.0[..6],
                                    got.1,
                                    &want.0[..6],
                                    want.1
                                ));
                            }
                            if got.3 != 1 {
                                counts.push(format!("{what}: {} replacements for one", got.3));
                            }
                        }
                    }
                }
            }
            assert_eq!(
                sized_here, 7075,
                "{gname} {amb:?}: the prototype's pair space"
            );
            sized += sized_here;
        }
    }
    assert_eq!(sized, 4 * 7075);
    // Positive control: the walk reached the four befores the decision is
    // about, in every ground and width.
    assert!(prepend_pairs > 0, "no Prepend case was reached");
    assert!(
        differ.is_empty(),
        "{} of {} pairs differ (every read size {sized}, every split {split}); \
         at read sizes and splits {tally:?}; the first:\n{}",
        differ.len(),
        sized + split,
        differ[..differ.len().min(8)].join("\n")
    );
    assert!(
        counts.is_empty(),
        "{} replacement counts wrong; the first:\n{}",
        counts.len(),
        counts[..counts.len().min(8)].join("\n")
    );
}

/// **A control between the Prepend and the bad bytes ends the cluster
/// before the U+FFFD**, as it ends it before a literal one (`[[source:
/// uax29]]` GB4/GB5): after `BEL` the U+FFFD takes the next cell, and after
/// `CUF` the cell past the one it skips. The sweep above cannot see this:
/// none of its befores ends in a control and a Prepend together, and it is
/// what holds the ground after a control to telling the cluster machine a
/// control went past before it writes the substitute.
///
/// A Prepend joins the cell before it (note AB's open GB9b departure), so
/// U+0600 is on the `a`.
#[test]
fn a_control_ends_the_cluster_before_a_replacement_for_ill_formed_bytes() {
    let mut checked = 0usize;
    for amb in [AmbiguousWidth::Narrow, AmbiguousWidth::Wide] {
        let fffd_cols = match amb {
            AmbiguousWidth::Narrow => 1,
            AmbiguousWidth::Wide => 2,
        };
        for (gname, enter, control, skip) in [
            ("ANSI", &b""[..], &b"\x07"[..], 0usize),
            ("ANSI", &b""[..], &b"\x1b[C"[..], 1),
            ("VT52", &b"\x1b[?2l"[..], &b"\x07"[..], 0),
            ("VT52", &b"\x1b[?2l"[..], &b"\x1bC"[..], 1),
        ] {
            for (bn, b) in bads().into_iter().chain([("EF BF BD", LITERAL.to_vec())]) {
                let case = [&b"a"[..], "\u{600}".as_bytes(), control, &b, b"Z"].concat();
                let bytes = [enter, &case].concat();
                let (sizes, splits) = cuts(case.len());
                for (cut, at) in sizes.iter().chain(splits.iter()) {
                    let at: Vec<usize> = core::iter::once(enter.len())
                        .filter(|&e| e > 0)
                        .chain(at.iter().map(|a| a + enter.len()))
                        .collect();
                    let got = seen(&bytes, &at, amb);
                    let what = format!("{gname} {amb:?}: a U+0600 {control:02X?} {bn} Z, {cut}");
                    let fffd = 1 + skip;
                    assert_eq!(got.0[0], "61+600", "{what}");
                    assert_eq!(
                        got.0[fffd], "FFFD",
                        "{what}: the U+FFFD in a cell of its own"
                    );
                    assert_eq!(got.0[fffd + fffd_cols], "5A", "{what}");
                    assert_eq!(
                        got.1,
                        (Row::ZERO, Col::at_index(fffd + fffd_cols + 1)),
                        "{what}: the cursor"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 0);
}

/// **The replacement count includes a U+FFFD that attaches.** After a
/// Prepend the substitute joins the cluster and takes no cell, and it was
/// counted only where it took one (`Paint::glyph`). The positive control is
/// the same bytes at the start of a line, where it takes a cell.
#[test]
fn a_replacement_that_attaches_is_counted() {
    for (name, bytes, want) in [
        (
            "a U+0600 80 Z",
            [&b"a"[..], "\u{600}".as_bytes(), &[0x80], b"Z"].concat(),
            1,
        ),
        (
            "a U+0600 80 80 Z",
            [&b"a"[..], "\u{600}".as_bytes(), &[0x80, 0x80], b"Z"].concat(),
            2,
        ),
        ("80 Z", [&[0x80][..], b"Z"].concat(), 1),
        (
            "a U+0600 EF BF BD Z",
            [&b"a"[..], "\u{600}".as_bytes(), &LITERAL[..], b"Z"].concat(),
            0,
        ),
        ("EF BF BD Z", [&LITERAL[..], b"Z"].concat(), 0),
    ] {
        let (sizes, splits) = cuts(bytes.len());
        for (cut, at) in sizes.iter().chain(splits.iter()) {
            assert_eq!(
                seen(&bytes, at, AmbiguousWidth::Narrow).3,
                want,
                "{name}, {cut}"
            );
        }
    }
}
