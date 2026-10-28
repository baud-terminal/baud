//! **The resize's measure against a full read of the ring.**
//!
//! At a resize that leaves the primary narrower than what its rows hold,
//! the core flags each row of the ring that holds anything past the new
//! edge and lowers the reach to the furthest any does
//! (`View::held_past_edge`). It used to read every row from the new edge to
//! the reach; it now reads a row clear past the old edge one cell deep on a
//! narrowing and not at all otherwise, trusting the flags the passes past
//! the edge kept since the last resize. That trust is the invariant this
//! file holds: whatever the flags, the counts and the reach say right after
//! a resize must be what reading every cell says
//! ([`PastEdgeCheck::exact`]), and between resizes no row flagged clear may
//! hold anything ([`PastEdgeCheck::sound`]).
//!
//! The oracle is [`Term::past_edge_check`], which reads every cell past the
//! live edge of every row to the pitch: the old measure's per-row read, run
//! over the whole row. So every assertion here is the change against the
//! measure it replaced, and red wherever a row's flag, the reach or the
//! count would come out other than the full read's.
//!
//! * the seven recordings of `tests/resize/`, after every step, and fed at
//!   several read sizes against the steps fed whole;
//! * generated programs with resizes interleaved -- narrowings and growths
//!   of a column or many, height-only and same-size resizes, runs of them --
//!   among coloured erases, long lines, wide characters, shell marks, the
//!   line moves, the scroll down at the top, `CSI 3 J` and `RIS`, on a ring a
//!   few rows deep so it wraps and the re-cut drops lines;
//! * one test a mechanism, each of which a broken form turns red.
//!
//! **Resize-sequence invariance**, as this file means it: the measure's
//! answer after a resize depends on the ring as it stands and on nothing
//! the earlier resizes left -- the flags they set are an input the measure
//! trusts, never a part of its answer -- so after any interleaving of
//! resizes and bytes the answer equals a fresh full read. A same-size
//! resize is not a no-op (it resets the scroll region), so it cannot mean
//! that inserting one changes nothing.

mod support;

use baud_core::*;
use proptest::prelude::*;
use std::hash::{Hash, Hasher};

/// One `tests/resize/` recording: its bytes and its steps.
struct Fixture {
    name: &'static str,
    raw: Vec<u8>,
    steps: Vec<(WinSize, usize)>,
}

const RECORDINGS: [&str; 7] = [
    "drag",
    "fresh-burst",
    "fresh-drag",
    "fresh-zoom",
    "live-drag",
    "short",
    "zoom",
];

fn load(name: &'static str) -> Fixture {
    let dir = format!("{}/../tests/resize", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read(format!("{dir}/{name}.raw")).expect("raw");
    let text = std::fs::read_to_string(format!("{dir}/{name}.steps")).expect("steps");
    let mut steps = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (size, len) = line.split_once(' ').expect("COLSxROWS <len>");
        let (c, r) = size.split_once('x').expect("COLSxROWS");
        steps.push((
            WinSize {
                ws_col: c.parse().expect("cols"),
                ws_row: r.parse().expect("rows"),
            },
            len.parse().expect("len"),
        ));
    }
    let owed: usize = steps.iter().map(|(_, n)| n).sum();
    assert_eq!(
        owed,
        raw.len(),
        "{name}: the steps do not cover the recording"
    );
    Fixture { name, raw, steps }
}

/// What a step leaves that the measure could move: the live surface's ring,
/// the rotation, the cursor and the check.
fn fingerprint(t: &Term<'_>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    zerocopy::IntoBytes::as_bytes(t.cells()).hash(&mut h);
    t.rotation().index().hash(&mut h);
    let (y, x) = t.cursor();
    (y.index(), x.index()).hash(&mut h);
    format!("{:?}", t.past_edge_check()).hash(&mut h);
    h.finish()
}

fn feed_at(t: &mut Term<'_>, bytes: &[u8], read: usize) {
    for chunk in bytes.chunks(read.max(1)) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
    }
}

/// What a replay saw of the measure's work, one a resize: whether the
/// flags were kept on entry, and of those the narrowings and the same-width
/// ones with a row held -- the shapes the measure now reads less for -- and
/// whether anything lay past the edge after.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seen {
    Kept,
    KeptNarrowing,
    KeptSameWidthHeld,
    PastAfter,
}

fn count(seen: &[Seen], what: Seen) -> Rows {
    Rows::of_count(seen.iter().filter(|s| **s == what).count())
}

/// Replays `f`, a step's bytes in reads of `read` (`None`: the step whole,
/// as the app hands it), asserting after every resize that the measure is
/// the full read and after every feed that the flags are sound; the
/// fingerprint after each step.
fn replay(f: &Fixture, read: Option<usize>, seen: &mut Vec<Seen>) -> Vec<u64> {
    let widest = f.steps.iter().map(|(w, _)| w.ws_col).max().expect("steps");
    let tallest = f.steps.iter().map(|(w, _)| w.ws_row).max().expect("steps");
    let max = WinSize {
        ws_col: widest,
        ws_row: tallest,
    };
    let mut owned = Owned::new(&max);
    let mut t = Term::new(owned.storage(), &f.steps[0].0);
    let mut at = 0;
    let mut prints = Vec::with_capacity(f.steps.len());
    for (i, (ws, len)) in f.steps.iter().enumerate() {
        let before = t.past_edge_check();
        let (cols, _) = t.geometry();
        if before.reach.0.index() > cols.index() {
            seen.push(Seen::Kept);
            match usize::from(ws.ws_col).cmp(&cols.index()) {
                std::cmp::Ordering::Less => seen.push(Seen::KeptNarrowing),
                std::cmp::Ordering::Equal if before.held.0.index() > 0 => {
                    seen.push(Seen::KeptSameWidthHeld)
                }
                _ => {}
            }
        }
        assert_eq!(t.resize(ws), Resized::Ok, "{}: step {i}", f.name);
        let after = t.past_edge_check();
        assert!(
            after.exact(),
            "{}: step {i} at {}x{}: the measure is not the full read: {after:?}",
            f.name,
            ws.ws_col,
            ws.ws_row
        );
        if after.reach.0.index() > usize::from(ws.ws_col) {
            seen.push(Seen::PastAfter);
        }
        let bytes = &f.raw[at..at + len];
        at += len;
        if !bytes.is_empty() {
            feed_at(&mut t, bytes, read.unwrap_or(bytes.len()));
            let fed = t.past_edge_check();
            assert!(
                fed.sound(),
                "{}: step {i}: a pass left the flags unsound: {fed:?}",
                f.name
            );
        }
        prints.push(fingerprint(&t));
    }
    prints
}

/// **Every recording, after every step: the measure is the full read.**
/// The positive control is the reach: across the seven, resizes were made
/// with the flags kept, narrowings among them, and same-width resizes with
/// a row held -- the paths the measure now reads less on -- so the check
/// is not passing on recordings that never reach them.
#[test]
fn every_recording_measures_as_the_full_read() {
    let mut seen = Vec::new();
    for name in RECORDINGS {
        let f = load(name);
        let mut here = Vec::new();
        replay(&f, None, &mut here);
        eprintln!(
            "{name}: {} steps, kept {:?}, kept narrowings {:?}, kept same width held {:?}, past after {:?}",
            f.steps.len(),
            count(&here, Seen::Kept),
            count(&here, Seen::KeptNarrowing),
            count(&here, Seen::KeptSameWidthHeld),
            count(&here, Seen::PastAfter)
        );
        seen.extend(here);
    }
    for what in [Seen::PastAfter, Seen::Kept, Seen::KeptNarrowing] {
        assert!(
            count(&seen, what) > Rows::ZERO,
            "no resize seen as {what:?}"
        );
    }
}

/// **And alike at six read sizes, against the steps fed whole**:
/// the screen, the rotation, the cursor and the check after every step.
#[test]
fn every_recording_measures_alike_at_six_read_sizes() {
    for name in RECORDINGS {
        let f = load(name);
        let whole = replay(&f, None, &mut Vec::new());
        for read in [1, 2, 3, 7, 61, 4096] {
            let split = replay(&f, Some(read), &mut Vec::new());
            let first = whole.iter().zip(&split).position(|(a, b)| a != b);
            assert_eq!(
                first, None,
                "{name}: reads of {read} differ from whole steps"
            );
        }
    }
}

// ---- generated programs -------------------------------------------------

/// The bytes a program is made of: what writes, erases or moves rows, in a
/// colour or not, on either side of the edge.
const ATOMS: &[&[u8]] = &[
    b"abcdefg",
    b"0123456789abcdefghijklmnopqrstuvwxyz0123456789ABCDEF",
    "\u{4e16}\u{754c}\u{4e16}\u{754c}\u{4e16}\u{754c}\u{4e16}\u{754c}\u{4e16}\u{754c}\u{4e16}"
        .as_bytes(),
    b"\r\n",
    b"\n",
    b"\x1bM",
    b"\x1b[44m\x1b[2K\x1b[0m",
    b"\x1b[44m\x1b[K\x1b[0m",
    b"\x1b[44m\x1b[J\x1b[0m",
    b"\x1b[44m\x1b[1J\x1b[0m",
    b"\x1b[44m\n\n\x1b[0m",
    b"\x1b[44m",
    b"\x1b[0m",
    b"\x1b[5X",
    b"\x1b[2L",
    b"\x1b[2M",
    b"\x1b[3P",
    b"\x1b[3@",
    b"\x1b[2;4r",
    b"\x1b[1;3r",
    b"\x1b[r",
    b"\x1b[1;1H",
    b"\x1b[3;9H",
    b"\x1b[99;1H",
    b"\x1b]133;A\x07$ ",
    b"\x1b]133;C\x07",
    b"\x1b]133;D\x07",
    b"\x1b[3J",
    b"\x1b[?3J",
    b"\x1bc",
    b"\x1b[2S",
    b"\x1b[2T",
    b"\x1b[?1049h",
    b"\x1b[?1049l",
    b"\x1b[?7l",
    b"\x1b[?7h",
    b"\x1b[2J",
    b"\x1b#8",
];

/// The largest screen a program resizes to, which the storage provisions.
const MAX: WinSize = WinSize {
    ws_col: 24,
    ws_row: 8,
};

#[derive(Clone, Debug)]
enum Op {
    /// An atom of [`ATOMS`].
    Bytes(usize),
    /// A resize to an absolute size.
    To(u16, u16),
    /// A resize by a step from the size in force, clamped: a drag's shape.
    By(i8, i8),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (0..ATOMS.len()).prop_map(Op::Bytes),
        1 => (2u16..=MAX.ws_col, 1u16..=MAX.ws_row).prop_map(|(c, r)| Op::To(c, r)),
        2 => (-3i8..=3, -1i8..=1).prop_map(|(c, r)| Op::By(c, r)),
    ]
}

/// A program, the ring's depth, and how many atoms go to a read in the
/// grouped run.
fn program() -> impl Strategy<Value = (Vec<Op>, usize, usize)> {
    (
        prop::collection::vec(op(), 1..80),
        prop_oneof![Just(0usize), Just(2), Just(5), Just(13)],
        1usize..=9,
    )
}

fn target(t: &Term<'_>, op: &Op) -> Option<WinSize> {
    let (cols, rows) = t.geometry();
    match *op {
        Op::Bytes(_) => None,
        Op::To(c, r) => Some(WinSize {
            ws_col: c,
            ws_row: r,
        }),
        Op::By(dc, dr) => {
            let step = |at: usize, by: i8, lo: u16, hi: u16| {
                let at = i32::try_from(at).expect("a size") + i32::from(by);
                u16::try_from(at.clamp(lo.into(), hi.into())).expect("clamped")
            };
            Some(WinSize {
                ws_col: step(cols.index(), dc, 2, MAX.ws_col),
                ws_row: step(rows.index(), dr, 1, MAX.ws_row),
            })
        }
    }
}

/// Runs a program, the atoms between two resizes fed `group` to a read
/// (`None`: each atom as its own read), checking after every resize and
/// every feed; the fingerprint after every resize, or the first failure.
///
/// **Grouped by atom, not cut at a byte**, because a read boundary between
/// a `CR` and its `LF` moves a wrap flag, which this core does before any
/// resize measures: `ESC [ 5 X`, a resize to 2 by 1, `CR` `LF` and twelve
/// wide characters leave the oldest line "Ended" when `CR` and `LF` are two
/// reads and "Continues" when they are one, on the commit before the
/// measure's. The recordings above are fed at byte read sizes, one
/// included.
fn run(prog: &[Op], depth: usize, group: Option<usize>) -> Result<Vec<u64>, String> {
    let mut owned = Owned::with_history(&MAX, &CacheGeometry::UNKNOWN, Depth::of(depth));
    let start = WinSize {
        ws_col: 20,
        ws_row: 6,
    };
    let mut t = Term::new(owned.storage(), &start);
    let mut prints = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let flush = |t: &mut Term<'_>, pending: &mut Vec<u8>| -> Result<(), String> {
        if pending.is_empty() {
            return Ok(());
        }
        feed_at(t, pending, pending.len());
        pending.clear();
        let c = t.past_edge_check();
        match c.sound() {
            true => Ok(()),
            false => Err(format!("unsound after bytes: {c:?}")),
        }
    };
    let mut held = 0;
    for (i, op) in prog.iter().enumerate() {
        match target(&t, op) {
            None => {
                let Op::Bytes(k) = *op else { unreachable!() };
                pending.extend_from_slice(ATOMS[k]);
                held += 1;
                if held >= group.unwrap_or(1) {
                    flush(&mut t, &mut pending)?;
                    held = 0;
                }
            }
            Some(ws) => {
                flush(&mut t, &mut pending)?;
                held = 0;
                assert_eq!(t.resize(&ws), Resized::Ok);
                let c = t.past_edge_check();
                if !c.exact() {
                    return Err(format!(
                        "op {i}, resize to {}x{}: {c:?}",
                        ws.ws_col, ws.ws_row
                    ));
                }
                prints.push(fingerprint(&t));
            }
        }
    }
    flush(&mut t, &mut pending)?;
    prints.push(fingerprint(&t));
    Ok(prints)
}

proptest! {
    /// **After any interleaving of resizes and bytes, the measure is the
    /// full read**: exact after every resize, sound after every feed.
    #[test]
    fn the_measure_is_the_full_read_after_any_interleaving((prog, depth, _) in program()) {
        if let Err(e) = run(&prog, depth, None) {
            prop_assert!(false, "{e}");
        }
    }

    /// **And the same whatever the reads**: the atoms between two resizes
    /// fed one to a read and fed several to a read leave the same ring,
    /// cursor and check after every resize.
    #[test]
    fn the_measure_is_chunk_invariant((prog, depth, group) in program()) {
        let whole = run(&prog, depth, None);
        let split = run(&prog, depth, Some(group));
        prop_assert!(whole.is_ok(), "{:?}", whole);
        prop_assert!(split.is_ok(), "{:?}", split);
        prop_assert_eq!(whole.unwrap(), split.unwrap());
    }
}

/// **The generator reaches the measure's three shapes**, counted over a
/// fixed sample: a narrowing that begins with the flags kept, a same-width
/// resize with a row held, and a narrowing in the shell's output, which
/// re-cuts. A property whose programs never reached them would pass
/// whatever the measure did.
#[test]
fn the_generator_reaches_the_measure() {
    use proptest::strategy::ValueTree;
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let (mut kept_narrowings, mut same_held, mut over_flagged) = (0, 0, 0);
    for _ in 0..400 {
        let (prog, depth, _) = program().new_tree(&mut runner).expect("tree").current();
        let mut owned = Owned::with_history(&MAX, &CacheGeometry::UNKNOWN, Depth::of(depth));
        let start = WinSize {
            ws_col: 20,
            ws_row: 6,
        };
        let mut t = Term::new(owned.storage(), &start);
        for op in &prog {
            match target(&t, op) {
                None => {
                    let Op::Bytes(k) = *op else { unreachable!() };
                    feed_at(&mut t, ATOMS[k], ATOMS[k].len());
                }
                Some(ws) => {
                    let c = t.past_edge_check();
                    let (cols, _) = t.geometry();
                    if c.reach.0.index() > cols.index() {
                        match usize::from(ws.ws_col).cmp(&cols.index()) {
                            std::cmp::Ordering::Less => kept_narrowings += 1,
                            std::cmp::Ordering::Equal if c.held.0.index() > 0 => {
                                same_held += 1;
                                if c.held_but_clear.is_some() {
                                    over_flagged += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    assert_eq!(t.resize(&ws), Resized::Ok);
                }
            }
        }
    }
    eprintln!(
        "kept narrowings {kept_narrowings}, same-width held {same_held} (over-flagged {over_flagged})"
    );
    assert!(kept_narrowings > 0);
    assert!(same_held > 0);
    assert!(over_flagged > 0);
}

// ---- one test a mechanism ----------------------------------------------

/// A terminal 40 by 10, with room for 12 rows and twenty lines of history;
/// `f` gets it and puts it at a prompt, so a resize re-cuts nothing.
fn at_prompt(f: impl FnOnce(&mut Term<'_>)) {
    let ws = WinSize {
        ws_col: 40,
        ws_row: 10,
    };
    let room = WinSize {
        ws_col: 40,
        ws_row: 12,
    };
    let mut owned = Owned::with_history(&room, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &ws);
    f(&mut t);
}

fn resize(t: &mut Term<'_>, cols: u16, rows: u16) {
    assert_eq!(
        t.resize(&WinSize {
            ws_col: cols,
            ws_row: rows,
        }),
        Resized::Ok
    );
}

/// Every read size and every two-read split of `bytes`, each from a fresh
/// terminal built by `setup`, through `then`.
fn every_split(bytes: &[u8], setup: impl Fn(&mut Term<'_>), then: impl Fn(&mut Term<'_>, &str)) {
    for read in 1..=bytes.len() {
        at_prompt(|t| {
            setup(t);
            feed_at(t, bytes, read);
            then(t, &format!("reads of {read}"));
        });
    }
    for cut in 1..bytes.len() {
        at_prompt(|t| {
            setup(t);
            feed_at(t, &bytes[..cut], cut);
            feed_at(t, &bytes[cut..], bytes.len() - cut);
            then(t, &format!("split at {cut}"));
        });
    }
}

/// The long line on row 2 that keeps the reach past the edge once the
/// screen narrows, so the flags are kept into the next resize.
const LONG: &[u8] = b"\x1b[2;1HLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL";

/// **A row clear at the old edge is held past the new one by its last
/// cell.** Narrowed to 30 at a prompt, with the long line held, a line is
/// erased in blue: blue from column 0 to 30 under a mark of zero, nothing
/// past 30, so it is clear there. Narrowed to 20, that row holds blue from
/// 20 to 30, which only its cell at 29 says -- its mark says nothing. The
/// control: the same row erased in the default blank stays clear.
#[test]
fn a_row_clear_at_the_old_edge_is_held_past_the_new_one_by_its_last_cell() {
    let prompt = b"\x1b[10;1H\x1b]133;A\x07$ ";
    every_split(
        b"\x1b[6;1H\x1b[44m\x1b[2K\x1b[0m\x1b[7;1H\x1b[2K\x1b[10;1H",
        |t| {
            support::feed(t, LONG);
            support::feed(t, prompt);
            resize(t, 30, 10);
            assert!(t.past_edge_check().exact());
        },
        |t, how| {
            assert!(t.past_edge_check().sound(), "{how}");
            resize(t, 20, 10);
            let c = t.past_edge_check();
            assert!(c.exact(), "{how}: {c:?}");
            // The long line, and the blue row: the control row is clear.
            assert_eq!(c.held.2, Rows::of_count(2), "{how}: {c:?}");
            assert_eq!(c.reach.0, Cols::of_index(38), "{how}: {c:?}");
        },
    );
}

/// **And by its mark when its last cell is blank.** Written while narrowed
/// to 30, twenty-five letters under a mark of 25, nothing past it: clear at
/// 30, its cell at 29 the default blank. Narrowed to 20 it holds the
/// letters from 20 to 25, which only its mark says.
#[test]
fn a_row_clear_at_the_old_edge_is_held_past_the_new_one_by_its_mark() {
    let prompt = b"\x1b[10;1H\x1b]133;A\x07$ ";
    every_split(
        b"\x1b[6;1Habcdefghijklmnopqrstuvwxy\x1b[10;1H",
        |t| {
            support::feed(t, LONG);
            support::feed(t, prompt);
            resize(t, 30, 10);
        },
        |t, how| {
            resize(t, 20, 10);
            let c = t.past_edge_check();
            assert!(c.exact(), "{how}: {c:?}");
            assert_eq!(c.held.2, Rows::of_count(2), "{how}: {c:?}");
            assert_eq!(c.reach.0, Cols::of_index(38), "{how}: {c:?}");
        },
    );
}

/// **A row held and emptied is read again at a resize of the height alone,
/// and at one to the same size.** Narrowed at a prompt, the long line is
/// held; erased while narrow by an `ECH` that reaches the edge, which runs
/// on to the mark, its row still says held -- the safe direction between
/// resizes -- and the reach lies past the edge. (An `EL` clears the flag
/// itself since it began to clear what the line held past the edge.) The
/// measure must read it again and find it clear, as the full read does:
/// every row clear, and the reach back at the live width.
#[test]
fn a_held_row_emptied_is_read_again_at_a_resize_of_the_height_or_the_same_size() {
    for (cols, rows) in [(20u16, 10u16), (20, 9), (20, 11)] {
        at_prompt(|t| {
            support::feed(t, LONG);
            support::feed(t, b"\x1b[10;1H\x1b]133;A\x07$ ");
            resize(t, 20, 10);
            support::feed(t, b"\x1b[2;1H\x1b[20X\x1b[10;3H");
            let before = t.past_edge_check();
            assert!(before.sound());
            assert!(before.held_but_clear.is_some(), "the control: {before:?}");
            resize(t, cols, rows);
            let c = t.past_edge_check();
            assert!(c.exact(), "{cols}x{rows}: {c:?}");
            assert_eq!(c.reach.0, Cols::of_index(20), "{cols}x{rows}: {c:?}");
        });
    }
}

/// **The flags left from an earlier narrowing are not trusted once the
/// reach came back to the live width.** Narrowed to 20 with the long line
/// held, then grown to 40: the reach is the live width and the flags are
/// no longer kept, but still say the long line's row is held and the rest
/// clear. Erased in blue while wide, a row is blue to 40; narrowed to 30,
/// it holds blue past the edge. Read from the stale flags it would be
/// clear; read as the reach says -- every row clear at 40 -- its last cell
/// holds it.
#[test]
fn stale_flags_are_not_trusted_once_the_reach_came_back() {
    at_prompt(|t| {
        support::feed(t, LONG);
        support::feed(t, b"\x1b[10;1H\x1b]133;A\x07$ ");
        resize(t, 20, 10);
        resize(t, 40, 10);
        support::feed(
            t,
            b"\x1b[2;1H\x1b[2K\x1b[6;1H\x1b[44m\x1b[2K\x1b[0m\x1b[10;3H",
        );
        assert_eq!(t.past_edge_check().reach.0, Cols::of_index(40));
        resize(t, 30, 10);
        let c = t.past_edge_check();
        assert!(c.exact(), "{c:?}");
        assert_eq!(c.held.2, Rows::of_count(1), "{c:?}");
        assert_eq!(c.reach.0, Cols::of_index(40), "{c:?}");
    });
}

/// **The re-cut's blanked rows are read whole.** A ring with no history
/// holding one line longer than itself: a narrowing re-cut drops the line
/// whole, and the window shows rows no re-cut line reached, blanked to the
/// new width under a mark of zero with the line's cells past it. A wide
/// character that could not take the last column leaves a blank there, so
/// the cell inside the old edge says nothing: only reading the row finds
/// them.
#[test]
fn the_rows_a_re_cut_blanked_are_read_whole() {
    let ws = WinSize {
        ws_col: 9,
        ws_row: 4,
    };
    let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::of(0));
    let mut t = Term::new(owned.storage(), &ws);
    // Four rows of nine, wrapped: four wide characters and a blank at the
    // last column on each row, so every row ends in a blank under its mark.
    let wide = "\u{4e16}".repeat(16);
    support::feed(&mut t, wide.as_bytes());
    resize(&mut t, 5, 4);
    let c = t.past_edge_check();
    assert!(c.exact(), "{c:?}");
}

/// **The program the property shrank to, pinned**: a line of 59 wrapped at
/// 20, re-cut to 4 and to 13 in the shell's output, then a prompt, a growth
/// to 17 at it, `ICH` moving two letters past the wrapped row's old mark, a
/// prompt's redraw over them, and a narrowing to 2. Before a wrapped row's
/// mark rose with a growth at a prompt, the redraw left the letters past a
/// mark of zero, and the one-cell read put the reach at 13 where the full
/// read finds 16.
#[test]
fn the_shrunk_program_measures_as_the_full_read() {
    let mut owned = Owned::with_history(&MAX, &CacheGeometry::UNKNOWN, Depth::of(13));
    let mut t = Term::new(
        owned.storage(),
        &WinSize {
            ws_col: 20,
            ws_row: 6,
        },
    );
    let steps: [(&[u8], (u16, u16)); 5] = [
        (
            b"0123456789abcdefghijklmnopqrstuvwxyz0123456789ABCDEFabcdefg",
            (4, 4),
        ),
        (b"\r\x1b[2;4r", (13, 1)),
        (b"\x1b]133;A\x07$ ", (17, 1)),
        (b"\x1b[3@\x1b]133;A\x07$ ", (2, 1)),
        (b"", (17, 1)),
    ];
    for (bytes, (c, r)) in steps {
        support::feed(&mut t, bytes);
        assert!(t.past_edge_check().sound());
        assert_eq!(
            t.resize(&WinSize {
                ws_col: c,
                ws_row: r,
            }),
            Resized::Ok
        );
        let check = t.past_edge_check();
        assert!(check.exact(), "{c}x{r}: {check:?}");
    }
}

/// **The program the property shrank to on 2026-10-14, pinned**: on the
/// alternate screen, a row erased in blue at 20 columns, narrowed to 19,
/// scrolled down to screen row 4, counted from zero (the fifth), cut by a
/// resize to 21 by 4 that grows the width, then a region of the top three
/// rows scrolled up. The growth's blank stopped at the cut row's mark, as a
/// fill to the end of a row does when its last cell is already the blank,
/// and left the blue cell at column 19 under a mark of 19; the scroll's
/// rotation brought that row in under the region, and the repair's
/// `copy_row` over it found it not uniform past its mark -- a panic in a
/// debug build. The visible half is `altresize.rs`'s
/// `a_band_past_the_old_edge_is_blanked_when_a_growth_reveals_it`.
#[test]
fn the_shrunk_alternate_growth_measures_as_the_full_read() {
    use Op::{By, Bytes, To};
    let prog = [
        Bytes(32),
        Bytes(22),
        Bytes(6),
        By(-1, 0),
        Bytes(31),
        To(21, 4),
        Bytes(19),
        Bytes(30),
    ];
    let r = run(&prog, 0, None);
    assert!(r.is_ok(), "{r:?}");
}
