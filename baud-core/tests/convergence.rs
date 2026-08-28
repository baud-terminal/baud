#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. That happened to
// `blanking_writes_the_whole_row_and_that_is_the_cheaper_choice`, whose
// assertion was wrong for two commits without anyone finding out. The
// compiler's own dead-code analysis is the check that catches it.
//! How fast do runs from different start states converge?
//!
//! `notes/b-data-parallel-fsm.md` proposes vectorising the state machine by
//! enumerating every possible start state at once -- Mytkowicz et al.'s method,
//! `[[source: dpfsm]]`. Chunk boundaries stop mattering because you compute
//! from all starts and keep the one that was right. It pays only if the
//! enumeration **collapses** quickly: once every start has reached the same
//! state, one lane is enough again.
//!
//! `[[source: dpfsm]]` § 5.2 reports convergence is common on their inputs.
//! Whether it holds on a VT stream was one of that note's open questions, and
//! this is the measurement. It matters because the other measurement -- the
//! path split -- says 57-90% of real bytes reach the state machine, so the
//! technique attacks the majority of the work if and only if it converges.

use baud_core::*;

/// Prefixes that leave the parser in distinct states.
///
/// Not every state: reaching `SosPmApcString` or `DcsPassthrough` needs a
/// string introducer, and those are here too. What matters is that the set is
/// *diverse* -- if they were all Ground the answer would be "converged at
/// byte 0" and would mean nothing.
const STARTS: [(&str, &[u8]); 8] = [
    ("ground", b""),
    ("escape", b"\x1b"),
    ("csi-entry", b"\x1b["),
    ("csi-params", b"\x1b[12;3"),
    ("csi-intermediate", b"\x1b[1 "),
    ("osc-string", b"\x1b]0;some title"),
    ("dcs-passthrough", b"\x1bP1;2|payload"),
    ("apc-string", b"\x1b_application command"),
];

fn feed(t: &mut Term, bytes: &[u8]) {
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }
}

/// Bytes until every start agrees, measured from `at` in `stream`.
///
/// `None` means they had not all converged within `limit`.
fn converge_at(stream: &[u8], at: usize, limit: usize) -> Option<usize> {
    let mut stores: Vec<Box<Storage>> = (0..STARTS.len()).map(|_| Storage::boxed()).collect();
    let mut terms: Vec<Term> = stores
        .iter_mut()
        .map(|s| Term::new(s, OpMode::Computed))
        .collect();
    for (t, (_, prefix)) in terms.iter_mut().zip(STARTS) {
        feed(t, prefix);
    }

    for k in 0..limit {
        let Some(b) = stream.get(at + k) else {
            return None;
        };
        for t in terms.iter_mut() {
            feed(t, &[*b]);
        }
        // Split so one term can be compared against the rest.
        let (head, rest) = terms.split_at(1);
        if rest.iter().all(|t| head[0].same_parser_state(t)) {
            return Some(k + 1);
        }
    }
    None
}

fn recording(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/recordings/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect(name)
}

/// **The starts are genuinely distinct before the stream begins.**
///
/// The control. If the prefixes all landed in the same state, every
/// convergence measurement below would read "1 byte" and mean nothing --
/// the probe would be unable to fire, which is this repo's recurring failure.
#[test]
fn the_start_states_differ_to_begin_with() {
    let mut stores: Vec<Box<Storage>> = (0..STARTS.len()).map(|_| Storage::boxed()).collect();
    let mut terms: Vec<Term> = stores
        .iter_mut()
        .map(|s| Term::new(s, OpMode::Computed))
        .collect();
    for (t, (_, prefix)) in terms.iter_mut().zip(STARTS) {
        feed(t, prefix);
    }
    let (head, rest) = terms.split_at(1);
    let agreeing = rest.iter().filter(|t| head[0].same_parser_state(t)).count();
    assert!(
        agreeing < rest.len(),
        "all {} starts are in the same state, so nothing here measures \
         convergence",
        STARTS.len()
    );
}

/// **Convergence on real capture, reported.**
///
/// Sampled across each recording rather than measured once, because
/// convergence is input-dependent: a run that begins inside a long printable
/// stretch converges differently from one that begins in dense escapes.
///
/// **Split by whether the stream contains an `ESC` at all, because that is the
/// whole answer and the old blanket assertion hid it.** Three of the eight
/// start states -- `osc-string`, `dcs-passthrough`, `apc-string` -- are inside
/// a control string, and a control string ends only at a string terminator.
/// A stream with no `ESC` byte therefore *cannot* collapse those lanes, ever,
/// however long it runs.
///
/// `cat-scroll.raw` and `cat-cjk.raw` are exactly that stream: 448 KB and 327
/// KB of text with **zero** `ESC` bytes between them. The old list happened to
/// name only btop, asciiquarium and vim, all escape-dense, so it asserted
/// "everything converges" while never testing anything that could not.
///
/// This matters beyond the technique. `cat`, logs and build output are the
/// workload baud is *fastest* at and the one `payload::streaming` models, and
/// on that workload the enumerative method has no amortisation at all -- not
/// "slow to converge" but 0 of 200 samples inside 4096 bytes. Note B already
/// rules the technique out on other grounds (`[[source: dpfsm]]` § 2.1 assumes
/// out-of-order action invocation, which a terminal cannot do); this is an
/// independent reason and a sharper one.
///
/// The escape-free case is **asserted rather than skipped**, so that a future
/// change which makes it converge shows up as a failure to explain rather than
/// passing silently.
#[test]
fn convergence_distance_on_real_capture() {
    // (recording, does it contain ESC)
    let corpus: [(&str, bool); 7] = [
        ("utf8/btop.raw", true),
        ("ascii/asciiquarium.raw", true),
        ("utf8/vim-unicode.raw", true),
        ("ascii/git-log.raw", true),
        ("utf8/less-cjk.raw", true),
        ("ascii/cat-scroll.raw", false),
        ("utf8/cat-cjk.raw", false),
    ];
    for (name, has_esc) in corpus {
        let d = recording(name);
        assert_eq!(
            d.contains(&0x1b),
            has_esc,
            "{name}: the table's claim about `ESC` disagrees with the file, so \
             the split below is measuring something other than it says"
        );
        let limit = 4096;
        let (mut converged, mut total, mut sum, mut worst) = (0usize, 0usize, 0usize, 0usize);
        let step = (d.len() - limit) / 200;
        for i in 0..200 {
            let at = 1024 + i * step;
            total += 1;
            if let Some(k) = converge_at(&d, at, limit) {
                converged += 1;
                sum += k;
                worst = worst.max(k);
            }
        }
        let pct = 100.0 * converged as f64 / total as f64;
        let mean = if converged > 0 {
            sum as f64 / converged as f64
        } else {
            0.0
        };
        println!(
            "{name:24} esc={has_esc:<5} converged {pct:5.1}% of {total}, \
             mean {mean:6.1} bytes, worst {worst}"
        );
        assert_eq!(total, 200, "the sampler stopped sampling");
        if has_esc {
            assert_eq!(
                converged, total,
                "{name}: {converged}/{total} samples converged within {limit} \
                 bytes. The enumerative method needs the lanes to collapse; a \
                 sample that never does is one where every start state stays \
                 distinct, and that is the case the technique cannot amortise."
            );
        } else {
            assert_eq!(
                converged, 0,
                "{name}: {converged}/{total} samples converged, but this \
                 stream has no `ESC` byte, so the three control-string start \
                 states have no terminator to reach and cannot collapse. \
                 Convergence here means the parser left a control string \
                 without a string terminator, which is a bug, not a win."
            );
        }
    }
}
