//! Every sequence `docs/vt-coverage.md` marks **done** reaches a real handler.
//!
//! `coverage.rs` checks that every done row is *mentioned* by a test. Mentioned
//! is not exercised: a row could name an operation, a test could name it in a
//! comment, and the sequence could still fall through dispatch to
//! `Csi::Unsupported`. That is precisely the `parsed` failure this document is
//! built around — "the terminal accepts a sequence, does nothing, and gives the
//! application no way to find out" — and nothing detected it for rows *already*
//! marked done.
//!
//! This closes that. Each row's sequence is synthesised from the table itself
//! and fed to a real `Term`; `Term::unsupported` must not move. It does not
//! prove the handler is *correct* — that is what the per-protocol suites are
//! for, and no mechanism can prove it in general — but it does prove the
//! parser reaches one, for every operation the table claims.

use baud_core::*;

/// Sequences the table names that cannot be synthesised from the row alone,
/// each with the reason and where it *is* covered.
///
/// Kept explicit and small, as in `coverage.rs`. A growing list means the check
/// is being avoided rather than an operation covered.
const UNSYNTHESISABLE: &[(&str, &str)] = &[
    // Parser states, not operations an application sends.
    ("ground", "a parser state"),
    ("escape", "a parser state"),
    ("escape_intermediate", "a parser state"),
    ("csi_entry", "a parser state"),
    ("csi_param", "a parser state"),
    ("csi_intermediate", "a parser state"),
    ("csi_ignore", "a parser state"),
    ("dcs_entry", "a parser state"),
    ("dcs_param", "a parser state"),
    ("dcs_intermediate", "a parser state"),
    ("dcs_passthrough", "a parser state"),
    ("dcs_ignore", "a parser state"),
    ("osc_string", "a parser state"),
    ("sos_pm_apc_string", "a parser state"),
];

/// Turn a table cell such as `CSI ? 2 l` or `ESC # 8` into wire bytes.
///
/// Parameters are supplied where the row writes none but the operation needs
/// one: a bare `CSI ? h` is not a sequence, it is a shape.
fn synthesise(seq: &str) -> Option<Vec<u8>> {
    let seq = seq.trim();
    if UNSYNTHESISABLE.iter().any(|(s, _)| *s == seq) {
        return None;
    }
    // Rows naming two forms (`CSI s` / `CSI u`) are exercised as the first.
    let seq = seq.split('/').next()?.trim();

    // Tokenised, not space-stripped: `SP` is the *name* of byte 0x20, and
    // stripping spaces turned `ESC SP F` into the three letters `S`, `P`, `F`.
    let mut out: Vec<u8> = Vec::new();
    let mut is_csi = false;
    for tok in seq.split_whitespace() {
        match tok {
            "ESC" => out.push(0x1b),
            "CSI" => {
                out.extend_from_slice(b"\x1b[");
                is_csi = true;
            }
            "SP" => out.push(0x20),
            _ => out.extend_from_slice(tok.as_bytes()),
        }
    }
    if out.len() < 2 {
        return None;
    }
    if !is_csi {
        return Some(out);
    }
    // A row written as a shape rather than a sequence -- `CSI ? h` -- needs a
    // parameter to be one. `1` is a real mode in the private namespace and the
    // default for every counted operation here.
    let head = 2 + usize::from(matches!(out.get(2), Some(b'?' | b'>' | b'<' | b'=')));
    if !out[head..].iter().any(|b| b.is_ascii_digit()) && head > 2 {
        out.insert(head, b'1');
    }
    Some(out)
}

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

/// Rows of `(sequence, status)` from the coverage table.
fn rows() -> Vec<(String, String)> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/vt-coverage.md"
    ))
    .expect("vt-coverage.md");
    let mut out = Vec::new();
    for line in src.lines() {
        let l = line.trim();
        if !l.starts_with("| `") {
            continue;
        }
        let cells: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() < 2 {
            continue;
        }
        let Some(seq) = first_backticked(cells[0]) else {
            continue;
        };
        let status = cells[cells.len() - 1].to_string();
        out.push((seq, status));
    }
    out
}

/// The first backticked span of a table cell.
///
/// Not `trim_matches('`')`: some rows carry a description after the sequence
/// (`` `ESC SP F` — S7C1T, send 7-bit controls ``), and HPA's final byte *is* a
/// backtick, written in markdown as `` \` ``. Both fooled the naive version
/// into synthesising the prose as wire bytes.
fn first_backticked(cell: &str) -> Option<String> {
    let b = cell.as_bytes();
    if b.first() != Some(&b'`') {
        return None;
    }
    let mut i = 1;
    let mut out = String::new();
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() && b[i + 1] == b'`' {
            out.push('`'); // an escaped backtick is a literal one
            i += 2;
            continue;
        }
        if b[i] == b'`' {
            return Some(out);
        }
        out.push(b[i] as char);
        i += 1;
    }
    None
}

/// **No sequence the table marks done is dropped as unsupported.**
#[test]
fn every_done_sequence_reaches_a_handler() {
    let mut dropped = Vec::new();
    let mut checked = 0usize;
    for (seq, status) in rows() {
        if !status.starts_with("done") {
            continue;
        }
        let Some(bytes) = synthesise(&seq) else {
            continue;
        };
        let mut s = Storage::boxed();
        let mut t = Term::new(&mut s, OpMode::Computed);
        feed(&mut t, &bytes);
        checked += 1;
        if t.unsupported.count() != 0 {
            dropped.push(format!("`{seq}` -> {bytes:02x?} counted as unsupported"));
        }
    }
    assert!(
        checked > 45,
        "only {checked} sequences synthesised -- the table parser stopped matching"
    );
    assert!(
        dropped.is_empty(),
        "{} done row(s) whose sequence never reaches a handler:\n  {}",
        dropped.len(),
        dropped.join("\n  ")
    );
}

/// The check can fail: a sequence the table does **not** carry is unsupported.
///
/// Without this, a `synthesise` that quietly returned `None` for everything
/// would leave the test above green while proving nothing — the
/// "instrumentation outlived what fed it" trap.
#[test]
fn the_handler_check_can_actually_fail() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    feed(&mut t, b"\x1b[1\x7e"); // `CSI 1 ~`, which baud does not implement
    assert_eq!(
        t.unsupported.count(),
        1,
        "an unimplemented final byte must be counted"
    );
}

/// **The standing refusals, each pinned to what it actually refuses.**
///
/// `CLAUDE.md` called six of these load-bearing. Four were closed once the
/// stated reason was read against its *scope* rather than as a settled fact —
/// the DEC glyph table, X10 mouse reports, counting the `OSC 52` request, and
/// `?1001` highlight tracking. In every case the reason survived and the scope
/// shrank to fit it: baud still does not guess a glyph, still refuses a
/// position X10 cannot name, still answers a clipboard read with silence, and
/// still does not draw a highlight.
///
/// The two that remain are not absences at all. DECRQM and DECRQSS are
/// implemented; what each refuses is a particular *answer*, and refusing it is
/// the correct behaviour. This records that, so the next reader does not have
/// to re-derive which kind they are.
#[test]
fn the_standing_refusals_are_what_they_claim() {
    let feed = |t: &mut Term, bytes: &[u8]| {
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(bytes));
        t.feed(base, f.len());
        t.commit_read(f.len());
    };

    // DECRQM answers `0` for a mode it does not have — not `2`, which would
    // say the mode exists and is merely off, whereupon the application would
    // set it and believe it took effect.
    let mut s1 = Storage::boxed();
    let mut t1 = Term::new(&mut s1, OpMode::Computed);
    feed(&mut t1, b"\x1b[?9999$p");
    assert_eq!(
        t1.reply(),
        b"\x1b[?9999;0$y",
        "DECRQM is implemented and answers `not recognized`"
    );

    // DECRQSS answers `1` for valid and `0` for invalid, the reverse of what
    // the DEC manuals say. ctlseqs records the errata and vttest agrees.
    let mut s2 = Storage::boxed();
    let mut t2 = Term::new(&mut s2, OpMode::Computed);
    feed(&mut t2, b"\x1bP$qm\x1b\\");
    let r = t2.reply();
    assert!(
        r.starts_with(b"\x1bP1$r"),
        "DECRQSS is implemented and answers 1 for a valid request, got {r:02x?}"
    );
}
