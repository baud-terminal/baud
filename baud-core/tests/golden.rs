//! What `baud-core2` rendered, asserted against this core now that it is gone.
//!
//! **This is the differential's coverage, frozen.** `baud-bench/tests/
//! differential.rs` drove both cores over the same bytes and compared all
//! 10,000 cells, and it is the instrument that found the `DECSTR`
//! alternate-screen bug, four marks bugs, HT, SO/SI, `ESC ( 0`, the
//! alternate-screen restore, `CAN`/`SUB` and the malformed-UTF-8 hang. Every
//! fixture in `golden/fixtures.txt` exists because it found something.
//!
//! **It is strictly weaker than what it replaces and the difference is worth
//! stating.** A differential finds a disagreement on *any* bytes; a golden file
//! finds one only on the bytes somebody wrote down. What it still does is stop
//! a regression on all of them, which is the half that survives having one core.
//!
//! The data is core2's output, captured by a one-off capture bin in the commit that
//! deleted it, which went with it. Nothing here re-derives it: if a value looks wrong,
//! it is what a mature implementation actually did, and that is the point.
//!
//! **Two departures, each a decision of this core's and labelled where it is
//! pinned.** The recordings' digests fold the resolved rendition rather than
//! core2's slot index since the style table became open-addressed, and
//! `utf8/cat-cjk`'s line moved on 2026-10-01 when a `U+FE0F` began widening
//! its base: those are this core's renderings, not core2's.

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeMap;

const CORPUS_COLS: usize = 200;

/// One fixture: the bytes, and what core2 made of them.
struct Fixture {
    name: String,
    bytes: Vec<u8>,
    cursor: (usize, usize),
    reply: Vec<u8>,
    cells: BTreeMap<(usize, usize), u32>,
    nonblank: NonBlank,
}

/// How many cells core2 left non-blank.
///
/// A newtype because it sits in the same struct as a `(row, col)` pair and a
/// reply, and a bare count there can be compared against a length that means
/// something else -- which is exactly the assertion this file makes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct NonBlank(usize);

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}

fn fixtures() -> Vec<Fixture> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/fixtures.txt");
    let src = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let mut out = Vec::new();
    let mut cur: Option<Fixture> = None;
    for line in src.lines() {
        let mut w = line.split(' ');
        match w.next() {
            Some("#") | Some("") | None => {}
            Some("fixture") => {
                cur = Some(Fixture {
                    name: w.collect::<Vec<_>>().join(" "),
                    bytes: Vec::new(),
                    cursor: (0, 0),
                    reply: Vec::new(),
                    cells: BTreeMap::new(),
                    nonblank: NonBlank(0),
                });
            }
            Some("bytes") => cur.as_mut().unwrap().bytes = hex(w.next().unwrap_or("")),
            Some("cursor") => {
                let f = cur.as_mut().unwrap();
                f.cursor = (
                    w.next().unwrap().parse().unwrap(),
                    w.next().unwrap().parse().unwrap(),
                );
            }
            Some("reply") => cur.as_mut().unwrap().reply = hex(w.next().unwrap_or("")),
            Some("digest") => {
                let f = cur.as_mut().unwrap();
                let _ = w.next();
                let _ = w.next();
                f.nonblank = NonBlank(w.next().unwrap().parse().unwrap());
            }
            Some("cell") => {
                let f = cur.as_mut().unwrap();
                let y: usize = w.next().unwrap().parse().unwrap();
                let x: usize = w.next().unwrap().parse().unwrap();
                let cp: u32 = w.next().unwrap().parse().unwrap();
                f.cells.insert((y, x), cp);
            }
            Some("end") => out.push(cur.take().unwrap()),
            Some(other) => panic!("golden: unknown key {other:?}"),
        }
    }
    // **A positive control.** An empty or truncated file would make every
    // assertion below vacuous, which is the probe-that-cannot-fire this repo
    // records nine times.
    assert!(
        out.len() > 40,
        "only {} fixtures parsed -- the golden file is truncated",
        out.len()
    );
    out
}

/// Every fixture renders the cells core2 rendered.
#[test]
fn every_fixture_renders_what_core2_rendered() {
    let mut checked = 0usize;
    for f in fixtures() {
        support::with_corpus(|t| {
            support::feed(t, &f.bytes);
            let w = t.stride().index();
            assert!(
                w >= CORPUS_COLS,
                "{}: the pitch admits the corpus width",
                f.name
            );
            let vp = t.viewport();
            let mut got = BTreeMap::new();
            for y in 0..50 {
                for x in 0..CORPUS_COLS {
                    let c = vp[y * w + x];
                    let v = match c.content.as_codepoint() {
                        Some(cp) => cp.value(),
                        None if c.content.is_empty() => continue,
                        None => 0,
                    };
                    got.insert((y, x), v);
                }
            }
            assert_eq!(
                got, f.cells,
                "{}: the cells differ from what core2 rendered",
                f.name
            );
            assert_eq!(
                NonBlank(got.len()),
                f.nonblank,
                "{}: non-blank count",
                f.name
            );
            checked += 1;
        });
    }
    assert!(checked > 40, "only {checked} fixtures driven");
}

/// And puts the cursor where core2 put it.
#[test]
fn every_fixture_leaves_the_cursor_where_core2_left_it() {
    for f in fixtures() {
        support::with_corpus(|t| {
            support::feed(t, &f.bytes);
            let (r, c) = t.cursor();
            assert_eq!((r.index(), c.index()), f.cursor, "{}: cursor", f.name);
        });
    }
}

/// And owes the host the same bytes.
///
/// The `DECRQSS` rows are the whole reason this is separate: they change no
/// cell, so the two assertions above are structurally blind to them.
#[test]
fn every_fixture_owes_the_same_reply() {
    let mut with_replies = 0usize;
    for f in fixtures() {
        support::with_corpus(|t| {
            support::feed(t, &f.bytes);
            assert_eq!(t.reply(), &f.reply[..], "{}: reply", f.name);
            if !f.reply.is_empty() {
                with_replies += 1;
            }
        });
    }
    // A positive control: if nothing replies, this test is asserting that
    // silence equals silence.
    assert!(
        with_replies > 10,
        "only {with_replies} fixtures reply -- this gate is checking nothing"
    );
}

/// Each recording renders to its pinned digest, over content and **resolved
/// rendition**.
///
/// A digest rather than a cell list because nine times ten thousand cells is
/// not a file anyone reads. It says *that* a rendering changed; the eleven
/// suites say what. It folds `style_of(cell)` -- the colour a renderer draws
/// -- and not `style.index()`, the slot in the rendition table: the slot is an
/// internal id that open-addressed interning reassigns without changing a
/// colour, and it is blind to the eviction bug the probe fixed, where a cell
/// kept its slot id while the slot's contents were overwritten. The digests
/// were recomputed from this core when the fold changed; core2 is gone.
#[test]
fn every_recording_renders_to_its_pinned_digest() {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/recordings.txt");
    let src = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let mut checked = 0usize;
    for line in src.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let w: Vec<&str> = line.split(' ').collect();
        let (rec, want, nonblank) = (w[0], w[2], NonBlank(w[4].parse().unwrap()));
        let rp = format!(
            "{}/../tests/recordings/{rec}.raw",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&rp).unwrap_or_else(|e| panic!("{rp}: {e}"));
        support::with_corpus(|t| {
            support::feed(t, &bytes);
            let w2 = t.stride().index();
            // The live cells, row by row: the pitch is wider than the screen
            // and the padding past each row is not a rendering.
            let vp = t.viewport();
            let cells: Vec<_> = (0..50)
                .flat_map(|y| vp[y * w2..y * w2 + CORPUS_COLS].to_vec())
                .collect();
            let mut digest = 0xcbf29ce484222325u64;
            let mut got = 0usize;
            for c in cells {
                let v = match c.content.as_codepoint() {
                    Some(cp) => cp.value(),
                    None if c.content.is_empty() => u32::MAX,
                    None => 0,
                };
                digest ^= u64::from(v);
                digest = digest.wrapping_mul(0x100000001b3);
                // The resolved rendition, not the slot: what a renderer draws.
                digest ^= t.style_of(c).key();
                digest = digest.wrapping_mul(0x100000001b3);
                if v != u32::MAX {
                    got += 1;
                }
            }
            assert_eq!(NonBlank(got), nonblank, "{rec}: non-blank cells");
            assert_eq!(
                format!("{digest:016x}"),
                want,
                "{rec}: the rendering changed -- {got} non-blank cells is right, \
                 so look at content or style rather than at layout"
            );
        });
        checked += 1;
    }
    assert_eq!(checked, 9, "all nine recordings");
}
