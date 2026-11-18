//! **What each target lacks, without a GPU**: [`Stale`] over the two
//! targets a window draws into in turn. A frame into a target draws every
//! row changed since that target's frame, whichever target or arm drew the
//! frames between; a frame the compositor did not take keeps its rows owed;
//! and the bands a partial frame draws are its runs, the closest joined.

#![cfg(target_os = "linux")]

use baud_domain::{Row, Rows};
use baud_present_vulkan::damage::{BANDS, Stale, TargetIx, bands};
use baud_render::{Redraw, RowSet};

const A: TargetIx = TargetIx(0);
const B: TargetIx = TargetIx(1);

fn rows(r: &[usize]) -> Redraw {
    r.iter()
        .fold(Redraw::NOTHING, |d, &i| d.with(Row::from_index(i)))
}

/// Both targets holding the frame drawn last: a whole frame into the
/// first, the same frame into the second.
fn both_hold() -> Stale {
    let mut s = Stale::new();
    assert_eq!(s.drew(A, Redraw::Whole), Redraw::Whole, "A held nothing");
    assert_eq!(s.drew(B, Redraw::NOTHING), Redraw::Whole, "B held nothing");
    s
}

/// **Three frames over two targets**: each draws the rows its target
/// missed while the other was drawn, and its own. Break: damage kept per
/// frame rather than per target, and the second target's draw is its own
/// frame's row 5 alone.
#[test]
fn a_target_draws_what_it_missed() {
    let mut s = both_hold();
    assert_eq!(s.drew(A, rows(&[2])), rows(&[2]), "A missed nothing");
    assert_eq!(s.owed(B), rows(&[2]), "B holds the frame before");
    assert_eq!(s.drew(B, rows(&[5])), rows(&[2, 5]), "B: both frames' rows");
    assert_eq!(
        s.drew(A, rows(&[7])),
        rows(&[5, 7]),
        "A: B's frame and its own"
    );
    assert_eq!(s.owed(A), Redraw::NOTHING);
    assert_eq!(s.owed(B), rows(&[7]));
}

/// **A frame the compositor did not take keeps its rows**: the next frame
/// into the same target draws them again, so the rows it hands the
/// compositor cover everything since the frame the compositor shows.
/// Break: the refused frame's rows dropped, and the next draws row 5 alone.
#[test]
fn a_refused_frame_keeps_its_rows() {
    let mut s = both_hold();
    assert_eq!(s.drew(A, rows(&[2])), rows(&[2]));
    s.refused(A);
    assert_eq!(s.owed(A), rows(&[2]), "the refused frame's rows, owed");
    assert_eq!(s.drew(A, rows(&[5])), rows(&[2, 5]), "drawn on the next");
    assert_eq!(s.owed(A), Redraw::NOTHING, "and then owed no more");
    // Refused twice running, the rows of both are owed.
    s.refused(A);
    assert_eq!(s.drew(A, rows(&[9])), rows(&[2, 5, 9]));
    s.refused(A);
    assert_eq!(s.owed(A), rows(&[2, 5, 9]));
    assert_eq!(s.owed(B), rows(&[2, 5, 9]), "B missed all three");
}

/// **Rows drawn on the CPU arm are stale on both targets**: the frames
/// went to shared memory, so neither target holds them, and the first GPU
/// frame into each draws them. Break: those rows never reaching a target,
/// and each GPU frame drawing its own row 6 alone.
#[test]
fn a_gpu_frame_after_the_cpu_arm_draws_what_it_missed() {
    let mut s = both_hold();
    s.elsewhere(rows(&[3]));
    s.elsewhere(rows(&[4]));
    assert_eq!(s.owed(A), rows(&[3, 4]));
    assert_eq!(s.owed(B), rows(&[3, 4]));
    assert_eq!(
        s.drew(A, rows(&[6])),
        rows(&[3, 4, 6]),
        "the first GPU frame"
    );
    assert_eq!(s.drew(B, Redraw::NOTHING), rows(&[3, 4, 6]), "the second");
    s.elsewhere(Redraw::Whole);
    assert_eq!(
        s.drew(A, Redraw::NOTHING),
        Redraw::Whole,
        "a whole CPU frame"
    );
}

/// A target that holds no frame, or lost the one it held, draws whole.
#[test]
fn a_forgotten_target_draws_whole() {
    let mut s = both_hold();
    s.forget(B);
    assert_eq!(s.draws(B, rows(&[1])), Redraw::Whole);
    assert_eq!(s.draws(A, rows(&[1])), rows(&[1]));
    assert_eq!(
        s.draws(TargetIx(2), rows(&[1])),
        Redraw::Whole,
        "no such target"
    );
}

/// **The bands are the runs, the closest joined past [`BANDS`]**: six runs
/// become four, the two narrowest gaps closed. Break: a join of the wrong
/// pair, which draws rows no run named.
#[test]
fn the_bands_join_the_closest_runs() {
    let set = [0, 1, 4, 10, 11, 13, 20, 30]
        .iter()
        .try_fold(RowSet::NONE, |s, &i| s.with(Row::from_index(i)))
        .expect("rows inside the set");
    let mut out = Vec::new();
    bands(set, &mut out);
    let got: Vec<(usize, usize)> = out.iter().map(|(r, n)| (r.index(), n.index())).collect();
    assert_eq!(got.len(), BANDS);
    // Runs 0-1, 4, 10-11, 13, 20, 30: gaps 2, 5, 1, 6, 9. The gap of one
    // joins 10-11 and 13, then the gap of two joins 0-1 and 4.
    assert_eq!(got, [(0, 5), (10, 4), (20, 1), (30, 1)]);
    let few = RowSet::NONE
        .span(Row::from_index(3), Row::from_index(5))
        .expect("rows");
    bands(few, &mut out);
    assert_eq!(out, [(Row::from_index(3), Rows::of_index(2))]);
}
