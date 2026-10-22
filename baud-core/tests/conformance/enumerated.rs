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

use crate::support;
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
    // A report baud sends, not a control it acts on: one a host sends is
    // counted (`a_received_declrp_is_counted`).
    ("CSI Pe ; Pb ; Pr ; Pc & w", "DECLRP, a report baud sends"),
];

/// Rows whose function `[[source: dec-std-070]]` recognises only below
/// Level 4, where baud powers up, each fed after the DECSCL that selects a
/// level that has it. Kept as small as `UNSYNTHESISABLE`.
const BELOW_LEVEL_FOUR: &[(&str, &[u8])] = &[
    // DECID, "Levels: 1, 2", and note b: "not recognized at Level 3 or
    // above" (the `ESC Z` row).
    ("ESC Z", b"\x1b[62;1\"p"),
];

/// Rows whose placeholders the default value cannot stand for, each with
/// the bytes it is fed as: a value the operation defines, where `1` is
/// not one or is another operation's. Kept as small as the lists above.
const REPRESENTATIVE: &[(&str, &[u8])] = &[
    // DECRQSS: `Pt` names a setting, SGR's.
    ("DCS $ q Pt ST", b"\x1bP$qm\x1b\\"),
    // DECSLPP is `Ps` 24 and up; below it the same final is a window
    // operation.
    ("CSI Ps t", b"\x1b[24t"),
    // A Sixel's data: one sixel.
    ("DCS Pa ; Pb ; Ph q ... ST", b"\x1bP0;1;0q~\x1b\\"),
    // DECRSPS: `Ps` 2, a DECTABSR's data, one stop.
    ("DCS Ps $ t D...D ST", b"\x1bP2$t9\x1b\\"),
    // OSC 46's `Pt` names a file, OSC 50's asks which font: `?`.
    ("OSC 46 ; Pt", b"\x1b]46;a.log\x1b\\"),
    ("OSC 50 ; Pt", b"\x1b]50;?\x1b\\"),
    // OSC 7's `Pt` is a `file:` URI; `1` is off its grammar, refused.
    ("OSC 7 ; Pt", b"\x1b]7;file:///tmp\x1b\\"),
    // MC's `Ps` 0, print the page: `1` is ECMA-48's transfer from an
    // auxiliary device, refused.
    ("CSI Ps i", b"\x1b[0i"),
    // The rows that name an `OSC` by its number alone: a body each
    // defines. The colours set one, `OSC 52` copies "hi", `OSC 3` names
    // the window.
    ("OSC 4", b"\x1b]4;1;rgb:ff/00/00\x1b\\"),
    ("OSC 5", b"\x1b]5;0;rgb:ff/00/00\x1b\\"),
    ("OSC 10", b"\x1b]10;rgb:ff/ff/ff\x1b\\"),
    ("OSC 12", b"\x1b]12;rgb:ff/ff/ff\x1b\\"),
    ("OSC 17", b"\x1b]17;rgb:ff/ff/ff\x1b\\"),
    ("OSC 52", b"\x1b]52;c;aGk=\x1b\\"),
    ("OSC 3 ; prop=value", b"\x1b]3;WM_NAME=x\x1b\\"),
    // And three whose bare number its handler passes over without a
    // count: a title, a link and a prompt mark each given.
    ("OSC 0/1/2", b"\x1b]0;t\x1b\\"),
    ("OSC 8", b"\x1b]8;;https://e.x\x1b\\"),
    ("OSC 133", b"\x1b]133;A\x1b\\"),
    // Progress at half, a kitty notification with no keys, and an
    // attention request: `st`, `metadata` and `Pv` name no value of
    // their own.
    ("OSC 9 ; 4 ; st ; pr", b"\x1b]9;4;1;50\x1b\\"),
    ("OSC 99 ; metadata ; payload", b"\x1b]99;;hi\x1b\\"),
    (
        "OSC 1337 ; RequestAttention=Pv",
        b"\x1b]1337;RequestAttention=once\x1b\\",
    ),
];

/// **A placeholder the table writes for a parameter** -- `Ps`, `Pn`,
/// `Pm`, `Pt`, and the named ones such as `Pts` or `Pch` -- each `P` and
/// lower-case letters, or `Ps..` for a list: `1`, a value every numeric
/// operation the table carries defines. Fed as the bytes `P` and `s`, a
/// row's `CSI Ps SP @` was `CSI P`, DCH, and the text after it, so every
/// such row passed whatever its handler did.
fn is_placeholder(word: &str) -> bool {
    let w = word.strip_suffix("..").unwrap_or(word);
    let mut c = w.chars();
    c.next() == Some('P') && w.len() > 1 && c.all(|c| c.is_ascii_lowercase())
}

/// One token's bytes, its placeholders `1`: a token is split on `;`, as
/// `Pt;Pl;Pb;Pr` writes four parameters in one.
fn fill(tok: &str, out: &mut Vec<u8>) {
    for (i, word) in tok.split(';').enumerate() {
        if i > 0 {
            out.push(b';');
        }
        match is_placeholder(word) {
            true => out.push(b'1'),
            false => out.extend_from_slice(word.as_bytes()),
        }
    }
}

/// Turn a table cell such as `CSI ? 2 l` or `ESC # 8` into wire bytes.
///
/// Parameters are supplied where the row writes none but the operation needs
/// one: a bare `CSI ? h` is not a sequence, it is a shape. A placeholder is
/// `1` ([`is_placeholder`]) unless [`REPRESENTATIVE`] gives the row; `OSC`,
/// `DCS` and `ST` are their bytes, and an `OSC` the row writes no end for
/// is ended with `ST`.
fn synthesise(seq: &str) -> Option<Vec<u8>> {
    let seq = seq.trim();
    if UNSYNTHESISABLE.iter().any(|(s, _)| *s == seq) {
        return None;
    }
    if let Some((_, bytes)) = REPRESENTATIVE.iter().find(|(s, _)| *s == seq) {
        return Some(bytes.to_vec());
    }
    // Rows naming two forms (`CSI s` / `CSI u`) are exercised as the first.
    let seq = seq.split('/').next()?.trim();

    // Tokenised, not space-stripped: `SP` is the *name* of byte 0x20, and
    // stripping spaces turned `ESC SP F` into the three letters `S`, `P`, `F`.
    let mut out: Vec<u8> = Vec::new();
    let mut is_csi = false;
    let mut is_osc = false;
    for tok in seq.split_whitespace() {
        match tok {
            "ESC" => out.push(0x1b),
            "CSI" => {
                out.extend_from_slice(b"\x1b[");
                is_csi = true;
            }
            "OSC" => {
                out.extend_from_slice(b"\x1b]");
                is_osc = true;
            }
            "DCS" => out.extend_from_slice(b"\x1bP"),
            "ST" => out.extend_from_slice(b"\x1b\\"),
            "SP" => out.push(0x20),
            _ => fill(tok, &mut out),
        }
    }
    if out.len() < 2 {
        return None;
    }
    if is_osc && !out.ends_with(b"\x1b\\") {
        out.extend_from_slice(b"\x1b\\");
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
    support::feed(t, bytes);
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
        let cells = cells(l);
        if cells.len() < 2 {
            continue;
        }
        let Some(seq) = first_backticked(&cells[0]) else {
            continue;
        };
        let status = cells[cells.len() - 1].clone();
        out.push((seq, status));
    }
    out
}

/// A table row's cells, split on the pipes that delimit them and not on an
/// escaped `\|`, which markdown renders as a literal pipe inside a cell.
///
/// Splitting on every `|` cut row 725 (`CSI ? 2031 h/l`) at the `\|` in its
/// status cell, so its last fragment, read as the status, did not start with
/// `done` and the row was skipped: a done row this check never fed. The same
/// split would drop `ESC |` (LS3R) from the sequence column entirely.
fn cells(row: &str) -> Vec<String> {
    let inner = row.trim().trim_start_matches('|');
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut out = vec![String::new()];
    let mut it = inner.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' if it.peek() == Some(&'|') => {
                out.last_mut().expect("one cell at least").push('|');
                it.next();
            }
            '|' => out.push(String::new()),
            c => out.last_mut().expect("one cell at least").push(c),
        }
    }
    out.iter().map(|c| c.trim().to_string()).collect()
}

/// **A cell holding an escaped pipe stays one cell.** Red while `rows()` split
/// on every `|`: row 725's status came back as the fragment after its `\|`.
#[test]
fn an_escaped_pipe_does_not_split_a_row() {
    let got = rows()
        .into_iter()
        .find(|(seq, _)| seq == "CSI ? 2031 h/l")
        .expect("the ?2031 row is read");
    assert!(
        got.1.starts_with("done"),
        "?2031's status was cut at an escaped pipe: {:?}",
        &got.1[..got.1.len().min(60)]
    );
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
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        if let Some((_, level)) = BELOW_LEVEL_FOUR.iter().find(|(r, _)| *r == seq) {
            feed(&mut t, level);
        }
        feed(&mut t, &bytes);
        checked += 1;
        if t.unsupported != 0 {
            dropped.push(format!("`{seq}` -> {bytes:02x?} counted as unsupported"));
        }
    }
    assert!(
        checked >= 230,
        "only {checked} sequences synthesised -- the table parser stopped matching"
    );
    assert!(
        dropped.is_empty(),
        "{} done row(s) whose sequence never reaches a handler:\n  {}",
        dropped.len(),
        dropped.join("\n  ")
    );
}

/// **A placeholder is fed as a parameter, and a string as its bytes.** Red
/// while `synthesise` passed `Ps` through as the bytes `P` and `s` --
/// `CSI Ps SP @` was `CSI P`, DCH, and the text ` @` -- and spelled `OSC`,
/// `DCS` and `ST` as letters, so every such row was fed as printable text
/// and passed whatever its handler did.
#[test]
fn a_placeholder_row_is_fed_as_its_sequence() {
    for (row, want) in [
        ("CSI Ps SP @", &b"\x1b[1 @"[..]),
        ("CSI Pn Y", b"\x1b[1Y"),
        ("CSI Pt;Pl;Pb;Pr $ z", b"\x1b[1;1;1;1$z"),
        ("CSI Pt;Pl;Pb;Pr;Ps.. $ r", b"\x1b[1;1;1;1;1$r"),
        ("CSI Ps ; Pu ' z", b"\x1b[1;1'z"),
        ("CSI 22 ; Ps t", b"\x1b[22;1t"),
        ("OSC 46 ; Pt", b"\x1b]46;a.log\x1b\\"),
        ("OSC 133", b"\x1b]133;A\x1b\\"),
        ("OSC 10", b"\x1b]10;rgb:ff/ff/ff\x1b\\"),
        ("OSC 104", b"\x1b]104\x1b\\"),
        ("DCS $ q Pt ST", b"\x1bP$qm\x1b\\"),
        ("Emoji_Presentation=Yes", b"Emoji_Presentation=Yes"),
    ] {
        assert_eq!(
            synthesise(row).as_deref(),
            Some(want),
            "{row}: {:?}",
            synthesise(row).map(|b| b.escape_ascii().to_string())
        );
    }
}

/// The check can fail: a sequence the table does **not** carry is unsupported.
///
/// Without this, a `synthesise` that quietly returned `None` for everything
/// would leave the test above green while proving nothing — the
/// "instrumentation outlived what fed it" trap.
#[test]
fn the_handler_check_can_actually_fail() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    feed(&mut t, b"\x1b[1\x7e"); // `CSI 1 ~`, which baud does not implement
    assert_eq!(
        t.unsupported, 1,
        "an unimplemented final byte must be counted"
    );
    // And for the two string families the table's `OSC` and `DCS` rows are
    // fed as: an `OSC` number and a `DCS` final baud has no handler for.
    for (what, bytes) in [
        ("OSC 9999", &b"\x1b]9999;x\x1b\\"[..]),
        ("DCS z", b"\x1bPz\x1b\\"),
    ] {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        feed(&mut t, bytes);
        assert_eq!(t.unsupported, 1, "{what} must be counted");
    }
}

/// **The standing refusals, each pinned to what it actually refuses.**
///
/// `CLAUDE.md` called six of these load-bearing. Four were closed once the
/// stated reason was read against its *scope* rather than as a settled fact —
/// the DEC glyph table, X10 mouse reports, counting the `OSC 52` request, and
/// `?1001` highlight tracking. In three the reason survived and the scope
/// shrank to fit it: baud still does not guess a glyph, still refuses a
/// position X10 cannot name, and still answers a clipboard read with silence.
/// The fourth outlived its reason: `?1001` is built to the spec, the region
/// drawn and the release reported, and nothing waits on the program's answer
/// (`tests/xterm/highlight.rs`).
///
/// The two that remain are not absences at all. DECRQM and DECRQSS are
/// implemented; what each refuses is a particular *answer*, and refusing it is
/// the correct behaviour. This records that, so the next reader does not have
/// to re-derive which kind they are.
#[test]
fn the_standing_refusals_are_what_they_claim() {
    let feed = |t: &mut Term, bytes: &[u8]| {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
    };

    // DECRQM answers `0` for a mode it does not have — not `2`, which would
    // say the mode exists and is merely off, whereupon the application would
    // set it and believe it took effect.
    let ws = support::CORPUS;
    let mut s1 = Owned::new(&ws);
    let mut t1 = Term::new(s1.storage(), &ws);
    feed(&mut t1, b"\x1b[?9999$p");
    assert_eq!(
        t1.reply(),
        b"\x1b[?9999;0$y",
        "DECRQM is implemented and answers `not recognized`"
    );

    // DECRQSS answers `1` for valid and `0` for invalid, the reverse of what
    // the DEC manuals say. ctlseqs records the errata and vttest agrees.
    let ws = support::CORPUS;
    let mut s2 = Owned::new(&ws);
    let mut t2 = Term::new(s2.storage(), &ws);
    feed(&mut t2, b"\x1bP$qm\x1b\\");
    let r = t2.reply();
    assert!(
        r.starts_with(b"\x1bP1$r"),
        "DECRQSS is implemented and answers 1 for a valid request, got {r:02x?}"
    );
}
