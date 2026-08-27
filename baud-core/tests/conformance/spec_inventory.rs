//! Every CSI final `[[source: ctlseqs]]` defines is accounted for.
//!
//! `docs/vt-coverage.md` is baud's own enumeration, and that is its weakness as
//! evidence: an operation the specification defines and baud never listed is
//! invisible in it. Not `todo`, not `out` -- absent. `coverage.rs` and
//! `enumerated.rs` both check the table against the code, and neither can see a
//! row that was never written.
//!
//! This checks the other direction: against the source rather than against
//! baud. ctlseqs documents 117 CSI sequences, many of them xterm extensions
//! baud deliberately does not implement, so the test is **not** that baud
//! handles them all. It is the weaker claim that actually matters --
//!
//! > every final byte the specification defines is either handled, or counted
//! > in `Term::unsupported`, and never silently consumed.
//!
//! Silent consumption is the `parsed` failure this project is built around:
//! "the terminal accepts a sequence, does nothing, and gives the application no
//! way to find out". Checked here against the specification's list, so a
//! sequence baud never thought about still cannot fall into it.

use baud_core::*;
use std::collections::BTreeSet;

/// Final bytes of the `CSI` entries in ctlseqs' "Functions using CSI" section.
fn spec_finals() -> BTreeSet<u8> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("ctlseqs");
    let mut out = BTreeSet::new();
    for line in src.lines() {
        if !line.starts_with("CSI ") {
            continue;
        }
        // The description begins after two spaces; the sequence precedes it.
        let seq = line.split("  ").next().unwrap_or(line).trim();
        if let Some(b) = plain_final(seq) {
            out.insert(b);
        }
    }
    out
}

/// Which finals `docs/vt-coverage.md` claims, in any namespace.
fn table_finals() -> BTreeSet<u8> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/vt-coverage.md"
    ))
    .expect("vt-coverage.md");
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let l = line.trim();
        if !l.starts_with("| `CSI") {
            continue;
        }
        let cell = l.trim_matches('|').split('|').next().unwrap_or("");
        // HPA's final byte *is* a backtick, written as a markdown escape.
        // Unescaped to a sentinel **before** splitting, because splitting on
        // backticks otherwise cuts the escape in half and yields the backslash
        // as the final byte.
        let cell = cell.replace("\\`", "\u{0}");
        // A cell may name several forms: `CSI H` / `CSI f`. Each backticked
        // span is one, and taking the last token of the joined string missed
        // `H` entirely.
        for form in cell.split('`').skip(1).step_by(2) {
            let form = form.replace('\u{0}', "`");
            if let Some(b) = plain_final(form.trim()) {
                out.insert(b);
            }
        }
    }
    out
}

/// The final byte of a **plain** `CSI` form: parameters only, no marker and no
/// intermediate.
///
/// Both directions need this and getting it wrong produced every one of the
/// first run's six failures. There is no bare `CSI p`: ctlseqs defines
/// `CSI ! p`, `CSI " p`, `CSI $ p`, `CSI # p` and `CSI > Ps p`, all of which
/// end in `p` and none of which is reached by `ESC [ p`. Taking the last token
/// regardless claimed the specification defined an operation it does not.
fn plain_final(seq: &str) -> Option<u8> {
    let rest = seq.strip_prefix("CSI")?;
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let (last, head) = toks.split_last()?;
    for t in head {
        // Parameter placeholders (`Ps`, `Pm`, `Pl`, `Pc`), separators and
        // literal digits are part of a plain form. Anything else -- a `?`, a
        // `>`, an `SP`, a `"` -- means this form is not one.
        let ok =
            t.starts_with('P') || *t == ";" || t.chars().all(|c| c.is_ascii_digit() || c == ';');
        if !ok {
            return None;
        }
    }
    let b = last.as_bytes();
    (b.len() == 1 && (0x40..=0x7e).contains(&b[0])).then(|| b[0])
}

fn unsupported_after(bytes: &[u8]) -> u64 {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(bytes));
    t.feed(base, f.len());
    t.commit_read(f.len());
    t.unsupported.count()
}

/// **No CSI final the specification defines is silently consumed.**
#[test]
fn every_spec_csi_final_is_handled_or_counted() {
    let spec = spec_finals();
    let table = table_finals();
    assert!(
        spec.len() > 30,
        "only {} finals parsed from ctlseqs -- the extractor stopped matching",
        spec.len()
    );
    assert!(
        table.len() > 20,
        "only {} finals parsed from vt-coverage.md",
        table.len()
    );
    let mut unaccounted = Vec::new();
    for b in &spec {
        let counted = unsupported_after(&[0x1b, b'[', *b]);
        let claimed = table.contains(b);
        if claimed && counted != 0 {
            unaccounted.push(format!(
                "CSI {} is in vt-coverage.md but reached Unsupported",
                *b as char
            ));
        }
        if !claimed && counted == 0 {
            unaccounted.push(format!(
                "CSI {} is defined by ctlseqs, absent from vt-coverage.md, and consumed in silence",
                *b as char
            ));
        }
    }
    assert!(
        unaccounted.is_empty(),
        "{} unaccounted CSI final(s) of {} in the specification:\n  {}",
        unaccounted.len(),
        spec.len(),
        unaccounted.join("\n  ")
    );
}
