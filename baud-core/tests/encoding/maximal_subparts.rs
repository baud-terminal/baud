//! **One U+FFFD per maximal subpart**, against `String::from_utf8_lossy`.
//!
//! `[[source: unicode-ch3]]` 3.9.5 requires only that an ill-formed
//! subsequence never swallow a successor that begins a well-formed one, and
//! leaves how many U+FFFD the subsequence itself becomes to the converter.
//! 3.9.6, "U+FFFD Substitution of Maximal Subparts", describes the practice
//! the W3C Encoding Standard adopted: "The maximal subpart at that offset is
//! replaced by a single U+FFFD", where a maximal subpart (D93b) is "the
//! initial subsequence of a well-formed code unit sequence, or ... a
//! subsequence of length one". Jev's CANCEL_AND_PRINT (decisions.jsonl lines
//! 451-452) says "one U+FFFD per maximal subpart", and that is the practice
//! built here. Rust's `from_utf8_lossy` implements the same practice, so it
//! is the oracle, and the first test holds it to the standard's own tables
//! before anything is judged against it.
//!
//! baud wrote one U+FFFD for a lead and every continuation after it, however
//! many maximal subparts they held: `E0 80` wrote one where the practice
//! writes two, because Table 3-7 lets `E0` continue only with `A0..BF`; and
//! `E0 80 80`, `ED A0 80` and `F4 90 80 80` wrote one where it writes three
//! or four.
//!
//! **What is compared is the screen.** Each case is fed raw, in every read
//! size and at every two-read split, and the screen is compared with the one
//! the same framing gives for the oracle's spelling of the same bytes: every
//! well-formed character as it is, and every maximal subpart as a lone `80`,
//! a stray continuation, which is the one byte every path replaces with
//! exactly one U+FFFD (`a_lone_continuation_is_one_replacement_everywhere`).
//! A literal `EF BF BD` stays itself. The spelling is fed in one read, so
//! the raw screen has to match it whatever the reads did.

use crate::support;
use baud_core::*;

const FFFD: u32 = 0xfffd;

/// Two rows of eighty: room for every framing below on row 0, and small
/// enough that the `RIS` between cases is cheap.
const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 80,
};

fn read(t: &mut Term<'_>, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Row 0 and the cursor: what a case left behind.
type Screen = (Vec<PackedCell>, (Row, Col));

fn screen(t: &Term<'_>) -> Screen {
    (
        t.row(Row::from_index(0))
            .expect("a row on the grid")
            .to_vec(),
        t.cursor(),
    )
}

/// `RIS`, then `setup` in a read of its own, then `case` in the reads
/// `cuts` gives -- each a length, the last running to the end.
///
/// VT52 has no `RIS`, so `ESC <` leaves it first; in ANSI it is a final
/// that does nothing.
fn run(t: &mut Term<'_>, setup: &[u8], case: &[u8], cuts: &[usize]) -> Screen {
    read(t, b"\x1b<\x1bc");
    read(t, setup);
    let mut at = 0;
    for &n in cuts {
        let end = (at + n).min(case.len());
        read(t, &case[at..end]);
        at = end;
    }
    read(t, &case[at..]);
    screen(t)
}

/// The oracle's spelling of `bytes`: each well-formed run as it is, each
/// maximal subpart as one `80`.
///
/// `Utf8Chunks` is what `from_utf8_lossy` is built on: one chunk's
/// `invalid()` is one maximal subpart, the bytes one U+FFFD replaces. That
/// the two agree is asserted here on every case, so the spelling is the
/// oracle and not a second opinion beside it.
fn spelled(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut lossy = String::new();
    for chunk in bytes.utf8_chunks() {
        out.extend_from_slice(chunk.valid().as_bytes());
        lossy.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            out.push(0x80);
            lossy.push('\u{fffd}');
        }
    }
    assert_eq!(lossy, String::from_utf8_lossy(bytes), "{bytes:02x?}");
    out
}

/// Every way to cut `case` into reads: every read size from one byte to
/// the whole, and every split into two reads.
fn cuttings(case: &[u8]) -> Vec<Vec<usize>> {
    let n = case.len();
    let mut v = Vec::new();
    for size in 1..=n.max(1) {
        v.push(vec![size; n.div_ceil(size)]);
    }
    for k in 1..n {
        v.push(vec![k]);
    }
    v
}

/// One context a probe is embedded in: what is fed before the case in a read
/// of its own, and the bytes around the probe inside it.
struct Framing {
    name: &'static str,
    setup: &'static [u8],
    before: &'static str,
    after: &'static str,
    /// Which probes the framing takes.
    probes: Probes,
}

/// After `ESC` an ASCII byte is a final, and the case would be about the
/// escape and not the text, so those framings take only probes that begin
/// with a byte `80-FF`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Probes {
    All,
    HighFirst,
}

const GROUND: Framing = Framing {
    name: "ground, after a letter",
    setup: b"",
    before: "a",
    after: "Z",
    probes: Probes::All,
};

/// Every framing: the ground, after a control, after `ESC` and after `ESC (`
/// (whose byte `80-FF` cancels and is the ground's), VT52, a run of each
/// encoded length the probe can continue or break, and a cluster. Each ends
/// in ASCII, which takes the text fold after a run.
///
/// The stride-three run is there twice, because it has two decoders: with
/// two characters before the probe and four after, the probe is the third
/// character of the first four, which `put_run` decodes in vectors once
/// sixteen bytes are in the read; with four before and one after, it is the
/// fifth, which the scalar tail decodes. Both runs are long enough for the
/// vector run scan, five characters a step.
const FRAMINGS: [Framing; 10] = [
    GROUND,
    Framing {
        name: "after a control",
        setup: b"",
        before: "\x07",
        after: "Z",
        probes: Probes::All,
    },
    Framing {
        name: "after ESC",
        setup: b"",
        before: "a\x1b",
        after: "Z",
        probes: Probes::HighFirst,
    },
    Framing {
        name: "after ESC (",
        setup: b"",
        before: "a\x1b(",
        after: "Z",
        probes: Probes::HighFirst,
    },
    Framing {
        name: "VT52",
        setup: b"\x1b[?2l",
        before: "a",
        after: "Z",
        probes: Probes::All,
    },
    Framing {
        name: "in a two-byte run",
        setup: b"",
        before: "\u{416}\u{416}",
        after: "\u{416}\u{416}Z",
        probes: Probes::All,
    },
    Framing {
        name: "in a three-byte run's vector decode",
        setup: b"",
        before: "\u{4e2d}\u{4e2d}",
        after: "\u{4e2d}\u{4e2d}\u{4e2d}\u{4e2d}Z",
        probes: Probes::All,
    },
    Framing {
        name: "in a three-byte run's scalar tail",
        setup: b"",
        before: "\u{4e2d}\u{4e2d}\u{4e2d}\u{4e2d}",
        after: "\u{4e2d}Z",
        probes: Probes::All,
    },
    Framing {
        name: "in a four-byte run",
        setup: b"",
        before: "\u{20000}\u{20000}",
        after: "\u{20000}Z",
        probes: Probes::All,
    },
    Framing {
        name: "in a cluster",
        setup: b"",
        before: "\u{1f469}\u{200d}",
        after: "\u{301}Z",
        probes: Probes::All,
    },
];

/// Feed `probe` in `f` raw at every cutting and compare each screen with the
/// oracle's. Returns the failures, described.
fn check(t: &mut Term<'_>, f: &Framing, probe: &[u8], out: &mut Vec<String>) {
    if f.probes == Probes::HighFirst && probe.first().is_none_or(|b| *b < 0x80) {
        return;
    }
    let case = [f.before.as_bytes(), probe, f.after.as_bytes()].concat();
    let want = run(t, f.setup, &spelled(&case), &[]);
    for cuts in cuttings(&case) {
        let got = run(t, f.setup, &case, &cuts);
        if got != want {
            out.push(format!(
                "{}: probe {probe:02x?} in reads {cuts:?}: {} cells, oracle {}",
                f.name,
                written(&got),
                written(&want),
            ));
            // One cutting is enough to name the probe.
            return;
        }
    }
}

/// A screen as the codepoints its cells hold, wide tails skipped, for a
/// failure message and for the position check.
fn codepoints(s: &Screen) -> Vec<u32> {
    let cols = s.1.1.index();
    s.0[..cols]
        .iter()
        .filter(|c| !c.content.is_wide_tail())
        .map(|c| c.content.as_codepoint().map_or(0, |c| c.value()))
        .collect()
}

fn written(s: &Screen) -> String {
    codepoints(s)
        .iter()
        .map(|c| match *c {
            FFFD => "FFFD".to_string(),
            c => format!("{c:04x}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn report(what: &str, fails: &[String]) {
    assert!(
        fails.is_empty(),
        "{what}: {} probes disagree with from_utf8_lossy:\n  {}",
        fails.len(),
        fails
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// **The oracle is the standard's practice, byte for byte.** Tables 3-8 to
/// 3-11 of `[[source: unicode-ch3]]` 3.9.6, as printed, against
/// `from_utf8_lossy`, and then against baud. A positive control: if the
/// oracle disagreed with the text, nothing below would mean anything.
#[test]
fn the_oracle_and_baud_give_tables_3_8_to_3_11() {
    let x = FFFD;
    let tables: [(&str, &[u8], &[u32]); 4] = [
        (
            "Table 3-8, non-shortest forms",
            &[0xc0, 0xaf, 0xe0, 0x80, 0xbf, 0xf0, 0x81, 0x82, 0x41],
            &[x, x, x, x, x, x, x, x, 0x41],
        ),
        (
            "Table 3-9, surrogates",
            &[0xed, 0xa0, 0x80, 0xed, 0xbf, 0xbf, 0xed, 0xaf, 0x41],
            &[x, x, x, x, x, x, x, x, 0x41],
        ),
        (
            "Table 3-10, other ill-formed sequences",
            &[0xf4, 0x91, 0x92, 0x93, 0xff, 0x41, 0x80, 0xbf, 0x42],
            &[x, x, x, x, x, 0x41, x, x, 0x42],
        ),
        (
            "Table 3-11, truncated sequences",
            &[0xe1, 0x80, 0xe2, 0xf0, 0x91, 0x92, 0xf1, 0xbf, 0x41],
            &[x, x, x, x, 0x41],
        ),
    ];
    support::with_term(&WS, |t| {
        for (name, bytes, want) in tables {
            let lossy: Vec<u32> = String::from_utf8_lossy(bytes)
                .chars()
                .map(u32::from)
                .collect();
            assert_eq!(lossy, want, "from_utf8_lossy, {name}");
            for cuts in cuttings(bytes) {
                let got = codepoints(&run(t, b"", bytes, &cuts));
                assert_eq!(got, want, "baud, {name}, reads {cuts:?}");
            }
        }
    });
}

/// **A lone `80` is one U+FFFD in every framing**, which is what lets the
/// oracle spell a maximal subpart as one: if it were not, the comparisons
/// below would be measuring the spelling.
#[test]
fn a_lone_continuation_is_one_replacement_everywhere() {
    support::with_term(&WS, |t| {
        for f in &FRAMINGS {
            let plain = [f.before.as_bytes(), f.after.as_bytes()].concat();
            let with = [f.before.as_bytes(), &[0x80], f.after.as_bytes()].concat();
            let a = codepoints(&run(t, f.setup, &plain, &[]));
            let b = codepoints(&run(t, f.setup, &with, &[]));
            let fffd = |v: &[u32]| v.iter().filter(|c| **c == FFFD).count();
            assert_eq!(fffd(&b), fffd(&a) + 1, "{}: {b:04x?}", f.name);
        }
    });
}

/// The byte classes of Table 3-7, one byte each, and the two the
/// implementation splits further. Continuations `80-8F`, `90-9F`, `A0-BF`;
/// ASCII; the leads `C2-DF`, `E0`, `E1-EC EE-EF`, `ED`, `F0`, `F1-F3`, `F4`;
/// and the bytes no sequence may begin with, `C0-C1` and `F5-FF`, with `F8`
/// beside `F5` because `F8-FF`'s low three bits alias a four-byte lead's.
/// Each was chosen so every well-formed sequence they spell is one
/// character in one cell or two, with no mark or cluster among them, which
/// is what lets the position check read the screen as characters.
const CLASSES: [u8; 14] = [
    0x85, 0x95, 0xb4, 0x41, 0xd0, 0xe0, 0xe2, 0xed, 0xf0, 0xf1, 0xf4, 0xc0, 0xf5, 0xf8,
];

/// Every probe of one to four bytes over `CLASSES`: 41,370 of them.
fn probes() -> Vec<Vec<u8>> {
    let mut all: Vec<Vec<u8>> = vec![vec![]];
    let mut out = Vec::new();
    for _ in 0..4 {
        let mut next = Vec::new();
        for p in &all {
            for b in CLASSES {
                let mut q = p.clone();
                q.push(b);
                next.push(q);
            }
        }
        out.extend(next.iter().cloned());
        all = next;
    }
    out
}

/// **Every one-to-four byte probe over the lead and continuation classes,
/// in the ground: the count and the positions of U+FFFD are
/// `from_utf8_lossy`'s.** Read off the screen as characters, so this is the
/// brief's claim stated directly, before the screen comparison below.
#[test]
fn every_probe_writes_the_replacements_from_utf8_lossy_writes() {
    let mut fails = Vec::new();
    support::with_term(&WS, |t| {
        for probe in probes() {
            let case = [b"a".as_slice(), &probe, b"Z"].concat();
            let want: Vec<u32> = String::from_utf8_lossy(&case)
                .chars()
                .map(u32::from)
                .collect();
            for cuts in cuttings(&case) {
                let got = codepoints(&run(t, b"", &case, &cuts));
                if got != want {
                    fails.push(format!(
                        "{probe:02x?} in reads {cuts:?}: {got:04x?}, from_utf8_lossy {want:04x?}"
                    ));
                    break;
                }
            }
        }
    });
    report("ground", &fails);
}

fn every_probe_in(f: &Framing) {
    let mut fails = Vec::new();
    support::with_term(&WS, |t| {
        for probe in probes() {
            check(t, f, &probe, &mut fails);
        }
    });
    report(f.name, &fails);
}

#[test]
fn every_probe_in_the_ground() {
    every_probe_in(&FRAMINGS[0]);
}

#[test]
fn every_probe_after_a_control() {
    every_probe_in(&FRAMINGS[1]);
}

#[test]
fn every_probe_after_esc() {
    every_probe_in(&FRAMINGS[2]);
}

#[test]
fn every_probe_after_esc_and_an_intermediate() {
    every_probe_in(&FRAMINGS[3]);
}

#[test]
fn every_probe_in_vt52() {
    every_probe_in(&FRAMINGS[4]);
}

#[test]
fn every_probe_in_a_two_byte_run() {
    every_probe_in(&FRAMINGS[5]);
}

#[test]
fn every_probe_in_a_three_byte_runs_vector_decode() {
    every_probe_in(&FRAMINGS[6]);
}

#[test]
fn every_probe_in_a_three_byte_runs_scalar_tail() {
    every_probe_in(&FRAMINGS[7]);
}

#[test]
fn every_probe_in_a_four_byte_run() {
    every_probe_in(&FRAMINGS[8]);
}

#[test]
fn every_probe_in_a_cluster() {
    every_probe_in(&FRAMINGS[9]);
}

/// **Every byte boundary of Table 3-7, not one byte a class.** The classes
/// above take one representative each, so an off-by-one at `9F`/`A0` or
/// `8F`/`90` would pass them. Here every lead `80-FF` meets every second
/// byte, every well-formed two-byte prefix every third byte, and every
/// well-formed three-byte prefix every fourth, each followed by two
/// continuations and a letter, in the ground and in a three-byte run.
fn boundary_probes() -> Vec<Vec<u8>> {
    let mut probes: Vec<Vec<u8>> = Vec::new();
    for lead in 0x80u8..=0xff {
        for second in 0u8..=0xff {
            probes.push(vec![lead, second]);
        }
    }
    for prefix in [
        &[0xe0, 0xa0][..],
        &[0xe0, 0xbf],
        &[0xe1, 0x80],
        &[0xec, 0xbf],
        &[0xed, 0x80],
        &[0xed, 0x9f],
        &[0xee, 0x80],
        &[0xef, 0xbf],
        &[0xf0, 0x90],
        &[0xf0, 0xbf],
        &[0xf1, 0x80],
        &[0xf3, 0xbf],
        &[0xf4, 0x80],
        &[0xf4, 0x8f],
        &[0xf0, 0x90, 0x80],
        &[0xf3, 0xbf, 0xbf],
        &[0xf4, 0x8f, 0xbf],
    ] {
        for b in 0u8..=0xff {
            probes.push([prefix, &[b]].concat());
        }
    }
    probes
        .into_iter()
        .map(|p| [p.as_slice(), &[0x85, 0x85]].concat())
        .collect()
}

fn every_boundary_in(f: &Framing) {
    let mut fails = Vec::new();
    support::with_term(&WS, |t| {
        for probe in boundary_probes() {
            check(t, f, &probe, &mut fails);
        }
    });
    report(f.name, &fails);
}

#[test]
fn every_lead_meets_every_byte_after_it_in_the_ground() {
    every_boundary_in(&FRAMINGS[0]);
}

#[test]
fn every_lead_meets_every_byte_after_it_in_a_three_byte_run() {
    every_boundary_in(&FRAMINGS[6]);
}

/// **Latin-1 is untouched**: every byte `80-FF` is one character there, so
/// the probes that are ill-formed UTF-8 write no U+FFFD at all.
#[test]
fn latin1_writes_no_replacement_for_any_probe() {
    support::with_term(&WS, |t| {
        for probe in probes() {
            let case = [b"a".as_slice(), &probe, b"Z"].concat();
            run(t, b"\x1b%@", &case, &[]);
            // Both rows and every cell: `85` is NEL under ISO 8859-1.
            let fffd = (0..2)
                .flat_map(|y| {
                    t.row(Row::from_index(y))
                        .expect("a row on the grid")
                        .to_vec()
                })
                .filter(|c| c.content.as_codepoint().map(|c| c.value()) == Some(FFFD))
                .count();
            assert_eq!(fffd, 0, "{probe:02x?} under ISO 8859-1");
        }
    });
}

/// **A byte no sequence may begin with never continues a run**, and the
/// four-byte case wrote a character. `run_of` admitted the next character of
/// a run when its lead claimed the run's length, and the route table gives
/// `C0-C1` two bytes and all of `F0-FF` four; a four-byte lead keeps its low
/// three bits, so `F8 A0 80 80` after U+20000 decoded as U+20000 again,
/// which conformance clause C10 forbids: an ill-formed sequence interpreted
/// as a character. `C0 80` after a two-byte character was one U+FFFD where
/// it is two. After one character the run scan's first test refuses the
/// byte, and after two its loop does; both are here.
#[test]
fn a_byte_no_sequence_begins_with_never_continues_a_run() {
    support::with_term(&WS, |t| {
        for (name, bytes) in [
            (
                "F8 A0 80 80 after U+20000",
                &b"\xf0\xa0\x80\x80\xf8\xa0\x80\x80Z"[..],
            ),
            (
                "F8 A0 80 80 after U+20000 twice",
                &b"\xf0\xa0\x80\x80\xf0\xa0\x80\x80\xf8\xa0\x80\x80Z"[..],
            ),
            (
                "C0 80 after U+0416 twice",
                &b"\xd0\x96\xd0\x96\xc0\x80Z"[..],
            ),
            (
                "FC A0 80 80 after U+20000",
                &b"\xf0\xa0\x80\x80\xfc\xa0\x80\x80Z"[..],
            ),
            (
                "F5 80 80 80 after U+20000",
                &b"\xf0\xa0\x80\x80\xf5\x80\x80\x80Z"[..],
            ),
            ("C0 80 after U+0416", &b"\xd0\x96\xc0\x80Z"[..]),
            ("C1 BF after U+0416", &b"\xd0\x96\xc1\xbfZ"[..]),
        ] {
            let want: Vec<u32> = String::from_utf8_lossy(bytes)
                .chars()
                .map(u32::from)
                .collect();
            for cuts in cuttings(bytes) {
                let got = codepoints(&run(t, b"", bytes, &cuts));
                assert_eq!(got, want, "{name}, reads {cuts:?}");
            }
        }
    });
}
