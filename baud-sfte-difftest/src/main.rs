//! The differential's driver: fixtures, recordings, pins and the verdict.
//! The shim, the two drivers and the comparison types are in `lib.rs`, so
//! `multibench` can reuse one description of how sfte is driven.
use baud_core::{Col, Owned, READ_CHUNK, Row};
use baud_sfte_difftest::*;

/// Where the first divergence at a boundary is, in a form a reader can act
/// on: which boundary, how many cells, and a bounding box in cells -- the
/// `diff_box` the pixel oracle grew because its first failure was unreadable
/// without one.
#[derive(Debug, Clone)]
struct Divergence {
    boundary: Boundary,
    cells: Cells,
    rows: (Row, Row),
    cols: (Col, Col),
    first: String,
    cursor: Option<((Row, Col), (Row, Col))>,
    reply: Option<(Vec<u8>, Vec<u8>)>,
}

fn diff(boundary: Boundary, a: &Screen, b: &Screen) -> Option<Divergence> {
    let mut n = Cells::NONE;
    let (mut r0, mut r1, mut c0, mut c1) = (usize::MAX, 0, usize::MAX, 0);
    let mut first = None;
    for (i, (x, y)) in a.cells.iter().zip(b.cells.iter()).enumerate() {
        if x != y {
            n.bump();
            let (r, c) = (i / cols(), i % cols());
            r0 = r0.min(r);
            r1 = r1.max(r);
            c0 = c0.min(c);
            c1 = c1.max(c);
            if first.is_none() {
                first = Some(format!("row {r} col {c}: ours {x:?} sfte {y:?}"));
            }
        }
    }
    let cursor = (a.cursor != b.cursor).then_some((a.cursor, b.cursor));
    let reply = (a.reply != b.reply).then(|| (a.reply.clone(), b.reply.clone()));
    if !n.any() && cursor.is_none() && reply.is_none() {
        return None;
    }
    Some(Divergence {
        boundary,
        cells: n,
        rows: (Row::from_index(r0), Row::from_index(r1)),
        cols: (Col::at_index(c0), Col::at_index(c1)),
        first: first.unwrap_or_default(),
        cursor,
        reply,
    })
}

/// One run's verdict: how many boundaries were compared, how many diverged,
/// and the first divergence in full.
struct Verdict {
    name: String,
    boundaries: Boundary,
    diverging: Boundary,
    cells_total: Cells,
    first: Option<Divergence>,
}

/// Drive both sides over `bytes` in `READ_CHUNK` pieces, comparing after each.
fn run(name: &str, bytes: &[u8], norm: &mut Normalised) -> Verdict {
    let mut owned = Owned::new(&WS);
    let mut ours = Ours::new(&mut owned);
    let mut theirs = Sfte::new();
    let mut boundaries = Boundary::NONE;
    let mut diverging = Boundary::NONE;
    let mut cells_total = Cells::NONE;
    let mut first = None;
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let chunk = &bytes[i..i + n];
        ours.feed(chunk);
        theirs.feed(chunk);
        i += n;
        boundaries.bump();
        let a = ours.screen();
        let b = theirs.screen(norm);
        if let Some(d) = diff(boundaries, &a, &b) {
            diverging.bump();
            cells_total = Cells::of(cells_total.get() + d.cells.get());
            if first.is_none() {
                first = Some(d);
            }
        }
    }
    Verdict {
        name: name.to_string(),
        boundaries,
        diverging,
        cells_total,
        first,
    }
}

/// The hand-written fixtures. Every case must either agree, or be listed in
/// `MUST_DIFFER` with the exact cells that differ and the reason.
fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("plain text", b"hello world".to_vec()),
        ("newline + cr", b"one\r\ntwo\r\nthree".to_vec()),
        ("SGR colour", b"\x1b[31mred\x1b[0m plain".to_vec()),
        ("cursor address", b"\x1b[5;10Hplaced".to_vec()),
        ("erase to EOL", b"abcdefgh\x1b[4G\x1b[K".to_vec()),
        ("erase screen", b"junk\x1b[2Jclean".to_vec()),
        ("erase chars", b"abcdefgh\x1b[3G\x1b[2X".to_vec()),
        ("cursor motion", b"abc\x1b[2D\x1b[Bx".to_vec()),
        ("wrap at margin", vec![b'x'; cols() + 10]),
        ("wrapped background", {
            let mut v = b"\x1b[48;2;255;0;0m".to_vec();
            v.extend(vec![b' '; cols() + 44]);
            v.extend(b"\x1b[0m\r\nnext");
            v
        }),
        (
            "erase-line bg",
            b"\x1b[48;2;0;0;255m\x1b[2K\x1b[0m\r\nafter".to_vec(),
        ),
        ("backspace", b"abc\x08\x08X".to_vec()),
        ("UTF-8 CJK", "日本語".as_bytes().to_vec()),
        ("wide at margin", {
            let mut v = vec![b'x'; cols() - 1];
            v.extend("日".as_bytes());
            v
        }),
        ("save/restore cur", b"\x1b7moved\x1b8back".to_vec()),
        ("reverse index", b"a\r\nb\x1bMx".to_vec()),
        ("DA reply", b"\x1b[c".to_vec()),
        ("DSR cursor reply", b"\x1b[3;7H\x1b[6n".to_vec()),
        (
            "alt screen in/out",
            b"main\x1b[?1049halt\x1b[?1049l".to_vec(),
        ),
        ("BEL-terminated OSC", b"\x1b]0;title\x07AFTER".to_vec()),
        // `?1049 h` while already on the alternate screen. xterm's set arm is
        // `CursorSave; ToAlternate; ClearScreen`, and `ClearScreen` runs
        // whatever buffer is up, so a second enter clears -- which sfte does
        // and baud has done since `96949c2`. alacritty gates `swap_alt` on
        // `!ALT_SCREEN` and keeps the screen. No recording re-enters, which is
        // why this is a fixture: it was found by `multibench` replaying
        // `less-pager.raw`, which never leaves the alternate screen, so its
        // second pass re-entered. The screens agree now, and the cursor is
        // the pinned difference -- see `MUST_DIFFER`.
        (
            "alt screen re-entry",
            b"\x1b[?1049hHELLO\x1b[?1049h".to_vec(),
        ),
        // `OSC 11` set then queried. sfte's setter is a no-op -- it always
        // answers its compile-time `SFTE_COLOR_BG`, black -- where baud
        // answers what was set. Probed through the patched header, because
        // under the pristine one a BEL-terminated query gets no reply at all.
        (
            "OSC 11 set then query",
            b"\x1b]11;rgb:aa/bb/cc\x07\x1b]11;?\x07".to_vec(),
        ),
        // An `ESC` abandons the escape or control sequence in progress and
        // starts a new one ([[source: williams]]'s anywhere edge, which sfte
        // does not follow -- see `MUST_DIFFER`): one case per node it abandons from.
        ("ESC in CSI params", b"\x1b[1\x1b[2mX".to_vec()),
        ("ESC in CSI, williams", b"\x1b[5;5H\x1b[3;1\x1b[HX".to_vec()),
        ("ESC after CSI", b"\x1b[\x1b[5;5HX".to_vec()),
        ("ESC in ignored CSI", b"\x1b[5;5H\x1b[1?\x1b[HX".to_vec()),
        ("ESC in escape", b"\x1b(\x1b0q".to_vec()),
        ("ESC after CSI inter", b"\x1b[ \x1b(0q".to_vec()),
        ("ESC after hash", b"\x1b7\x1b[5;5H\x1b#\x1b8X".to_vec()),
        // The two that MUST differ -- see `MUST_DIFFER`.
        ("tab", b"a\tb\tc".to_vec()),
        ("tab at a stop", b"01234567\tX".to_vec()),
    ]
}

/// Fixtures whose disagreement is the positive control, each pinned at the
/// exact values rather than at a count. A dead harness returning zeros cannot
/// produce these specific values on both sides, which is what makes them a
/// control rather than a known difference: they say the comparison is reading
/// both grids, both cursors and both reply buffers.
///
/// * **`tab`, `tab at a stop` -- sfte's HT lands one column past every stop**
///   and is a no-op at the last column: its HT handler
///   (sfte_dev.h:6005-6008) post-increments inside its test, so from column 0
///   it stops at 1, from 1-7 at 9, from 8 at 9 -- where baud stops at 8 and 16
///   (`baud-core/src/ops.rs`).
/// * **`DA reply` -- sfte answers `CSI ? 62 c`, a VT220; baud answers
///   `CSI ? 6 c`.** baud under-claims as a VT102 on purpose
///   (`docs/vt-coverage.md:542`): an application parses whatever arrives as
///   the report it asked for, and claiming a VT220's feature set would be a
///   lie it then acts on. Both replies are pinned byte for byte.
/// * **`alt screen re-entry` -- sfte homes the cursor on a second `?1049 h`,
///   and baud leaves it.** Both clear the screen, so every cell agrees; baud
///   clears without homing because xterm's `ClearScreen` does not home
///   (`xterm-util.c`), so after `HELLO` baud's cursor is at (0, 5) and sfte's
///   at (0, 0). Until `96949c2` baud did not clear at all, and the five cells
///   of `HELLO` were the difference pinned here.
/// * **`OSC 11 set then query` -- sfte's dynamic-colour setter is a no-op.**
///   It answers every query with its compile-time default (`SFTE_COLOR_BG`,
///   black, as `rgb:0000/0000/0000`), whatever `OSC 11` set before; baud
///   answers the value set, in the two-digit form. Both replies are pinned
///   byte for byte. It also says sfte *does* answer a fresh query, which baud
///   does only once the embedder has told it the theme.
/// * **`alt screen in/out` -- sfte does not restore the cursor on
///   `?1049 l`.** baud's `?1049` saves and restores it (`docs/vt-coverage.md:
///   675`; `?47` at `:573` is the variant that does not), so after `main`,
///   enter, `alt`, leave, baud is back at (0, 4) and sfte is at (1, 0) -- the
///   row the alternate screen's `alt` plus its blank left it on.
/// * **The seven `ESC` cases -- sfte does not abandon a sequence on `ESC`.**
///   Its CSI entry and parameter states ignore `1B`, a `[` final is a no-op,
///   its charset state eats any byte and its `#` state goes to ground on
///   anything but `8` (sfte.h ~35037-35103), so it prints what baud, vte and
///   `[[source: williams]]` treat as a new sequence. baud had the same
///   defect until the anywhere `1B` edge went in, and these agreed then.
enum Control {
    Cells(&'static [(usize, usize, char, char)]),
    Cursor((usize, usize), (usize, usize)), // (row, col) indices; a const table cannot call Row::from_index
    Reply(&'static [u8], &'static [u8]),
}

const MUST_DIFFER: &[(&str, Control)] = &[
    (
        "tab",
        Control::Cells(&[
            (0, 8, 'b', ' '),
            (0, 9, ' ', 'b'),
            (0, 16, 'c', ' '),
            (0, 17, ' ', 'c'),
        ]),
    ),
    (
        "tab at a stop",
        Control::Cells(&[(0, 9, ' ', 'X'), (0, 16, 'X', ' ')]),
    ),
    ("DA reply", Control::Reply(b"\x1b[?6c", b"\x1b[?62c")),
    ("alt screen in/out", Control::Cursor((0, 4), (1, 0))),
    ("alt screen re-entry", Control::Cursor((0, 5), (0, 0))),
    (
        "OSC 11 set then query",
        Control::Reply(
            b"\x1b]11;rgb:aaaa/bbbb/cccc\x07",
            b"\x1b]11;rgb:0000/0000/0000\x07",
        ),
    ),
    (
        "ESC in CSI params",
        Control::Cells(&[(0, 0, 'X', '2'), (0, 1, ' ', 'm'), (0, 2, ' ', 'X')]),
    ),
    (
        "ESC in CSI, williams",
        Control::Cells(&[(0, 0, 'X', ' '), (4, 4, ' ', 'H'), (4, 5, ' ', 'X')]),
    ),
    (
        "ESC after CSI",
        Control::Cells(&[
            (0, 0, ' ', '5'),
            (0, 1, ' ', ';'),
            (0, 2, ' ', '5'),
            (0, 3, ' ', 'H'),
            (0, 4, ' ', 'X'),
            (4, 4, 'X', ' '),
        ]),
    ),
    (
        "ESC in ignored CSI",
        Control::Cells(&[(0, 0, 'X', ' '), (4, 4, ' ', 'H'), (4, 5, ' ', 'X')]),
    ),
    (
        "ESC in escape",
        Control::Cells(&[(0, 0, 'q', '0'), (0, 1, ' ', 'q')]),
    ),
    (
        "ESC after CSI inter",
        Control::Cells(&[(0, 0, '\u{2500}', ' ')]),
    ),
    (
        "ESC after hash",
        Control::Cells(&[(0, 0, 'X', ' '), (4, 4, ' ', '8'), (4, 5, ' ', 'X')]),
    ),
];

/// A cursor as the `(row, col)` index pair a `const` control can spell.
fn at((r, c): (Row, Col)) -> (usize, usize) {
    (r.index(), c.index())
}

/// Whether one fixture's result matches its control exactly. `None` means it
/// does; `Some` carries what was wrong. Shared by `main` and the test so the
/// binary's verdict is as strong as the test's.
fn check_control(name: &str, want: &Control, a: &Screen, b: &Screen) -> Option<String> {
    let got: Vec<(usize, usize, char, char)> = a
        .cells
        .iter()
        .zip(b.cells.iter())
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .map(|(i, (x, y))| (i / cols(), i % cols(), x.ch, y.ch))
        .collect();
    match want {
        Control::Cells(w) if got.as_slice() != *w => {
            Some(format!("{name}: cells {got:?}, pinned {w:?}"))
        }
        Control::Cells(_) => None,
        Control::Cursor(oc, sc) => {
            if !got.is_empty() {
                Some(format!(
                    "{name}: cells differ and only the cursor was pinned: {got:?}"
                ))
            } else if (at(a.cursor), at(b.cursor)) != (*oc, *sc) {
                Some(format!(
                    "{name}: cursor ours {:?} sfte {:?}, pinned {oc:?} / {sc:?}",
                    at(a.cursor),
                    at(b.cursor)
                ))
            } else {
                None
            }
        }
        Control::Reply(or, sr) => {
            if !got.is_empty() {
                Some(format!(
                    "{name}: cells differ and only the reply was pinned: {got:?}"
                ))
            } else if (a.reply.as_slice(), b.reply.as_slice()) != (*or, *sr) {
                Some(format!(
                    "{name}: reply ours {:?} sfte {:?}, pinned {or:?} / {sr:?}",
                    a.reply, b.reply
                ))
            } else {
                None
            }
        }
    }
}

/// Run one fixture whole (one boundary) and return both screens.
fn once(bytes: &[u8], norm: &mut Normalised) -> (Screen, Screen) {
    let mut owned = Owned::new(&WS);
    let mut ours = Ours::new(&mut owned);
    let mut theirs = Sfte::new();
    ours.feed(bytes);
    theirs.feed(bytes);
    (ours.screen(), theirs.screen(norm))
}

/// The recordings, each pinned. Zero means asserted-zero; a non-zero pin is
/// the divergence's **cell count at its first diverging boundary** together
/// with the number of diverging boundaries, with the reason beside it -- so a
/// count that moves in either direction has to come and edit this table.
struct Pin {
    path: &'static str,
    diverging_boundaries: Boundary,
    first_cells: Cells,
    reason: &'static str,
}

fn pins() -> Vec<Pin> {
    // Six of nine agree at every boundary, cava included -- the recording the
    // survey had excluded on OSC 4, which this comparison never resolves, and
    // on a BEL-terminated OSC that sfte now handles through patch 0001. The
    // three that diverge do so for ONE reason, read off the differing cells
    // rather than assumed: sfte has no UAX #29 grapheme clustering. A ZWJ
    // family is one cluster in two cells for baud and four separate people in
    // eight cells for sfte; a regional-indicator pair is one flag against two;
    // a Devanagari combining mark attaches to its base in baud and takes its
    // own cell in sfte, shifting everything after it on the row by one. That
    // is a feature baud has and sfte does not, not a defect in either, and the
    // pin holds the count so the day it moves somebody reads why.
    const CLUSTERING: &str = "sfte has no UAX #29: ZWJ families, flags and \
                              combining marks each take their own cells";
    vec![
        Pin {
            path: "ascii/asciiquarium.raw",
            diverging_boundaries: Boundary::of(0),
            first_cells: Cells::of(0),
            reason: "pure ASCII",
        },
        Pin {
            path: "ascii/cat-scroll.raw",
            diverging_boundaries: Boundary::of(0),
            first_cells: Cells::of(0),
            reason: "",
        },
        Pin {
            path: "ascii/git-log.raw",
            diverging_boundaries: Boundary::of(0),
            first_cells: Cells::of(0),
            reason: "",
        },
        Pin {
            path: "ascii/less-pager.raw",
            diverging_boundaries: Boundary::of(0),
            first_cells: Cells::of(0),
            reason: "",
        },
        Pin {
            path: "utf8/btop.raw",
            diverging_boundaries: Boundary::of(0),
            first_cells: Cells::of(0),
            reason: "",
        },
        Pin {
            path: "utf8/cat-cjk.raw",
            diverging_boundaries: Boundary::of(80),
            first_cells: Cells::of(173),
            reason: CLUSTERING,
        },
        Pin {
            path: "utf8/cava.raw",
            diverging_boundaries: Boundary::of(0),
            first_cells: Cells::of(0),
            reason: "",
        },
        Pin {
            path: "utf8/less-cjk.raw",
            diverging_boundaries: Boundary::of(21),
            first_cells: Cells::of(173),
            reason: CLUSTERING,
        },
        Pin {
            path: "utf8/vim-unicode.raw",
            diverging_boundaries: Boundary::of(33),
            first_cells: Cells::of(137),
            reason: CLUSTERING,
        },
    ]
}

fn recording(path: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/recordings")
        .join(path);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn describe(d: &Divergence) -> String {
    let mut s = if d.cells.any() {
        format!(
            "boundary {}: {} cells, rows {}..={} cols {}..={}; first {}",
            d.boundary.get(),
            d.cells.get(),
            d.rows.0.index(),
            d.rows.1.index(),
            d.cols.0.index(),
            d.cols.1.index(),
            d.first
        )
    } else {
        format!("boundary {}: cells agree", d.boundary.get())
    };
    if let Some((a, b)) = d.cursor {
        s.push_str(&format!("; cursor ours {:?} sfte {:?}", at(a), at(b)));
    }
    if let Some((a, b)) = &d.reply {
        s.push_str(&format!("; reply ours {a:?} sfte {b:?}"));
    }
    s
}

/// Every run, so `main` and the test cannot drive different ones.
fn compare() -> (Vec<Verdict>, Vec<Verdict>, Normalised) {
    let mut norm = Normalised::new();
    let fx: Vec<Verdict> = fixtures()
        .into_iter()
        .map(|(name, bytes)| run(name, &bytes, &mut norm))
        .collect();
    let rec: Vec<Verdict> = pins()
        .iter()
        .map(|p| run(p.path, &recording(p.path), &mut norm))
        .collect();
    (fx, rec, norm)
}

fn main() {
    let locale = set_locale();
    let cell_bytes = unsafe { ffi::sfte_shim_cell_bytes() };
    println!(
        "  sfte: locale {locale}, cell {cell_bytes} bytes, chunk {READ_CHUNK}, {}x{}",
        cols(),
        rows()
    );
    let (fx, rec, norm) = compare();
    let mut fail = 0;
    println!(
        "\n  {:<24} {:>6} {:>6}  first divergence",
        "fixture", "bounds", "diverg"
    );
    for v in &fx {
        let must = MUST_DIFFER.iter().any(|(n, _)| *n == v.name);
        match &v.first {
            None if !must => println!("  {:<24} {:>6} {:>6}  ok", v.name, v.boundaries.get(), 0),
            None => {
                fail += 1;
                println!(
                    "  {:<24} {:>6} {:>6}  MUST DIFFER and did not",
                    v.name,
                    v.boundaries.get(),
                    0
                )
            }
            Some(d) if must => println!(
                "  {:<24} {:>6} {:>6}  differs as pinned: {}",
                v.name,
                v.boundaries.get(),
                v.diverging.get(),
                describe(d)
            ),
            Some(d) => {
                fail += 1;
                println!(
                    "  {:<24} {:>6} {:>6}  {}",
                    v.name,
                    v.boundaries.get(),
                    v.diverging.get(),
                    describe(d)
                )
            }
        }
    }
    // The controls' exact values, not only that they differ.
    let mut cnorm = Normalised::new();
    for (name, want) in MUST_DIFFER {
        let bytes = fixtures().into_iter().find(|(n, _)| n == name).unwrap().1;
        let (a, b) = once(&bytes, &mut cnorm);
        if let Some(why) = check_control(name, want, &a, &b) {
            fail += 1;
            println!("  CONTROL OFF ITS PIN: {why}");
        }
    }
    println!(
        "\n  {:<24} {:>6} {:>6} {:>8}  first divergence",
        "recording", "bounds", "diverg", "cells"
    );
    for (v, p) in rec.iter().zip(pins()) {
        let first_cells = v.first.as_ref().map_or(Cells::NONE, |d| d.cells);
        let pinned = v.diverging == p.diverging_boundaries && first_cells == p.first_cells;
        if !pinned {
            fail += 1;
        }
        match &v.first {
            None => println!(
                "  {:<24} {:>6} {:>6} {:>8}  ok{}",
                v.name,
                v.boundaries.get(),
                0,
                0,
                if pinned { "" } else { "  (pin says otherwise)" }
            ),
            Some(d) => println!(
                "  {:<24} {:>6} {:>6} {:>8}  {}{}",
                v.name,
                v.boundaries.get(),
                v.diverging.get(),
                v.cells_total.get(),
                describe(d),
                if pinned {
                    format!("  [pinned: {}]", p.reason)
                } else {
                    "  UNPINNED".to_string()
                }
            ),
        }
    }
    println!(
        "\n  normalised: {} wide tails, {} zero-rune blanks",
        norm.tails.get(),
        norm.blanks.get()
    );
    // **An oracle that only prints cannot fail.**
    if fail != 0 {
        println!("\n  {fail} unpinned");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Being a workspace member makes `cargo test --workspace` *build* this
    /// crate; this test is what makes it *run*.
    #[test]
    fn every_fixture_agrees_with_sfte_except_the_pinned_controls() {
        set_locale();
        let (fx, _, _) = compare();
        let mut bad = Vec::new();
        for v in &fx {
            let must = MUST_DIFFER.iter().find(|(n, _)| *n == v.name);
            match (must, &v.first) {
                (None, None) => {}
                (None, Some(d)) => bad.push(format!("{}: {}", v.name, describe(d))),
                (Some(_), None) => bad.push(format!("{}: MUST DIFFER and did not", v.name)),
                (Some(_), Some(_)) => {}
            }
        }
        assert!(
            bad.is_empty(),
            "baud-core and sfte disagree:\n  {}",
            bad.join("\n  ")
        );
    }

    /// **The positive control asserts exact values, not a count.** A count
    /// could be met by a dead harness that happened to differ somewhere; four
    /// named cells with the right characters on each side, a byte-exact reply
    /// pair, or a cursor pair can only come from reading both implementations.
    #[test]
    fn the_must_differ_fixtures_differ_at_exactly_the_pinned_values() {
        set_locale();
        let mut norm = Normalised::new();
        for (name, want) in MUST_DIFFER {
            let bytes = fixtures().into_iter().find(|(n, _)| n == name).unwrap().1;
            let (a, b) = once(&bytes, &mut norm);
            if let Some(why) = check_control(name, want, &a, &b) {
                panic!("{why}");
            }
        }
    }

    /// The recordings, every one, pinned.
    #[test]
    fn every_recording_matches_its_pin() {
        set_locale();
        let (_, rec, norm) = compare();
        let mut bad = Vec::new();
        for (v, p) in rec.iter().zip(pins()) {
            let first_cells = v.first.as_ref().map_or(Cells::NONE, |d| d.cells);
            if v.diverging != p.diverging_boundaries || first_cells != p.first_cells {
                bad.push(format!(
                    "{}: {} of {} boundaries diverge, first has {} cells (pin: {} boundaries, {} cells){}",
                    v.name,
                    v.diverging.get(),
                    v.boundaries.get(),
                    first_cells.get(),
                    p.diverging_boundaries.get(),
                    p.first_cells.get(),
                    v.first.as_ref().map_or(String::new(), |d| format!("\n      {}", describe(d)))
                ));
            }
        }
        assert!(
            bad.is_empty(),
            "recordings off their pins:\n  {}",
            bad.join("\n  ")
        );
        // **The normalisations are pinned exactly, not bounded**, so one that
        // starts firing somewhere new has to come and change these numbers.
        // Over every fixture and every boundary of every recording: 84,557
        // wide tails (every second cell of a double-width character, on both
        // sides, keyed on sfte's DUMMY flag) and 240,317 zero-rune blanks
        // (cells sfte has never written, read as blank -- the blindness the
        // `Normalised` doc records).
        assert_eq!(norm.tails.get(), 84_557, "wide tails normalised");
        // 240,317: it was 230,317 until the `OSC 11 set then query` fixture,
        // whose 10,000 untouched cells are exactly the difference. 310,301
        // since the seven `ESC` fixtures, measured: 69,984 more, the cells of
        // seven 200x50 screens that neither side wrote.
        assert_eq!(norm.blanks.get(), 310_301, "zero-rune blanks normalised");
    }

    /// The geometry assert can fire: the shim refuses a context at any other
    /// size, and this checks the Rust side would notice a lie about the size.
    #[test]
    fn the_shim_reports_the_corpus_geometry() {
        let s = Sfte::new();
        let (c, r) = unsafe { (ffi::sfte_shim_cols(s.s), ffi::sfte_shim_rows(s.s)) };
        assert_eq!((c, r), (200, 50));
    }
}

#[cfg(test)]
mod replay {
    use super::*;

    /// **A recording played twice is a case no single pass can reach.** Every
    /// recording here is truncated mid-session, so a second pass opens where
    /// the first left off -- inside whatever mode, screen or region the
    /// program was in. `less-pager.raw` and `btop.raw` never leave the
    /// alternate screen, so their second passes re-enter it.
    ///
    /// **Until `96949c2` sfte cleared on re-entry and baud did not**, and the
    /// consequence showed only mid-pass: two whole passes agreed, and
    /// `multibench`'s 32 MiB total cut less-pager's last pass at byte 1,934,
    /// where 155 cells of sfte's cleared screen had not yet been redrawn.
    /// baud clears on re-entry now, as xterm does, and that cut agrees.
    ///
    /// **What differs now is the colour the clear fills with, and only on
    /// btop.** xterm's `ClearScreen` fills with `ClearCurBackground` -- the
    /// background in force -- and so does baud's; sfte's fills with the
    /// default. btop's first pass ends inside a coloured rendition, so its
    /// second pass re-enters under a set background, and 2,800 cells differ
    /// in their background and in no character. The other five re-enter
    /// under the default, or not at all, and agree.
    #[test]
    fn a_second_pass_over_every_recording_is_pinned() {
        set_locale();
        let whole: &[&str] = &[
            "ascii/asciiquarium.raw",
            "ascii/cat-scroll.raw",
            "ascii/git-log.raw",
            "ascii/less-pager.raw",
            "utf8/btop.raw",
            "utf8/cava.raw",
        ];
        let mut bad = Vec::new();
        for path in whole {
            // `(cells differing, of which in their character)`.
            let pin = if *path == "utf8/btop.raw" {
                (2_800, 0)
            } else {
                (0, 0)
            };
            let got = replayed(path, 2, None);
            if got != pin {
                bad.push(format!(
                    "{path}: {got:?} cells differ after two whole passes (pin {pin:?})"
                ));
            }
        }
        // The cut multibench's replay lands on: 1,113 whole passes and then
        // 1,934 bytes. One whole pass plus the cut reproduces it exactly, and
        // the count was stable from 1 to 1,113 passes when bisected. 155 cells
        // until `96949c2`, when baud began clearing on re-entry as sfte does.
        let cut = replayed("ascii/less-pager.raw", 1, Some(1_934));
        if cut != (0, 0) {
            bad.push(format!(
                "ascii/less-pager.raw: {cut:?} cells differ one pass plus 1,934 bytes in (pin (0, 0))"
            ));
        }
        assert!(bad.is_empty(), "second-pass pins:\n  {}", bad.join("\n  "));
    }

    /// Feed `passes` whole passes of a recording, then `extra` bytes of the
    /// next, and count the cells that differ, and of those the ones whose
    /// character differs.
    fn replayed(path: &str, passes: usize, extra: Option<usize>) -> (usize, usize) {
        let bytes = recording(path);
        let mut owned = Owned::new(&WS);
        let mut ours = Ours::new(&mut owned);
        let mut theirs = Sfte::new();
        let mut norm = Normalised::new();
        let mut feed = |b: &[u8]| {
            let mut i = 0;
            while i < b.len() {
                let n = READ_CHUNK.min(b.len() - i);
                ours.feed(&b[i..i + n]);
                theirs.feed(&b[i..i + n]);
                i += n;
            }
        };
        for _ in 0..passes {
            feed(&bytes);
        }
        if let Some(e) = extra {
            feed(&bytes[..e]);
        }
        let (a, b) = (ours.screen(), theirs.screen(&mut norm));
        let differ = |f: fn(&Cell, &Cell) -> bool| {
            a.cells
                .iter()
                .zip(b.cells.iter())
                .filter(|(x, y)| f(x, y))
                .count()
        };
        (differ(|x, y| x != y), differ(|x, y| x.ch != y.ch))
    }
}
