//! The UFO logo's planner (`src/ufo.rs`): its seam and its depth, each
//! check the planner it was designed with asserted before it wrote a pose,
//! as a test, and the depth rules that planner's table keeps; its move
//! table; and every one of its 320 poses against the poses that planner
//! wrote, `ufo-poses.tsv`, the poses the film was designed with.

use baud_pictures::ufo::{
    self, BEHIND, BOB_S, DART_S, FLIP, FRAMES, FRONT, LOOP, MOVES, Plan, Pose, RIM_HZ, WOBBLE_S,
    frame_t,
};

/// The pose's channels the seam must close, by name.
fn closing(p: &Pose) -> [(&'static str, f64); 7] {
    [
        ("x", p.cx),
        ("y", p.cy),
        ("bank", p.bank),
        ("beam", p.beam),
        ("block beam", p.block_beam),
        ("lift", p.lift),
        ("depth", p.depth),
    ]
}

/// **The moves end inside the loop**: the last, the dart home and its
/// levelling, ends by 9.7 s, so the hover that closes the loop is not
/// negative. Breaks when a hold or a tilt grows past what the loop has
/// room for.
#[test]
fn the_moves_end_inside_the_loop() {
    let plan = Plan::build();
    assert!(plan.end <= LOOP + 1e-9, "the moves end at {}", plan.end);
    assert!((plan.end - 9.7).abs() < 1e-9, "they end at {}", plan.end);
}

/// **The loop closes on itself**: at 0 s and at 10 s the saucer is in the
/// same place, level, behind the word, both beams off and the block down.
/// Breaks when a move does not come home, a beam or the lift is left on at
/// the wrap, or the saucer ends the loop on the other side of the letters.
#[test]
fn the_loop_closes_on_itself() {
    let plan = Plan::build();
    let (a, b) = (plan.pose(0.0), plan.pose(LOOP));
    for ((name, x), (_, y)) in closing(&a).into_iter().zip(closing(&b)) {
        assert!((x - y).abs() < 1e-6, "{name}: {x} at 0 s, {y} at {LOOP} s");
    }
    assert_eq!(
        (a.beam, a.block_beam, a.lift, a.depth),
        (0.0, 0.0, 0.0, BEHIND),
        "dark, down, behind"
    );
}

/// **The rim lights turn a whole number of times a loop**: ten, eight at
/// 0.8 turns a second and two more while excited. Breaks when the rate or
/// the extra turns stop adding to whole turns, which the rim's lights would
/// show as a jump at the wrap.
#[test]
fn the_rim_makes_whole_turns() {
    let plan = Plan::build();
    let turns = plan.pose(LOOP).rim - plan.pose(0.0).rim;
    assert!((turns - turns.round()).abs() < 1e-9, "{turns} turns");
    assert_eq!(turns.round(), 10.0);
}

/// **Every period of the idle layer divides the loop**: the bob's 2.5 s,
/// the wobble's 5 s and the rim's 1.25 s. Breaks when one of them would
/// end the loop part-way through a cycle.
#[test]
fn the_idle_periods_divide_the_loop() {
    for per in [BOB_S, WOBBLE_S, 1.0 / RIM_HZ] {
        let n = LOOP / per;
        assert!((n - n.round()).abs() < 1e-9, "{per} s: {n} a loop");
    }
}

/// **No beam is lit during a dart**: each of the nine darts' start, end
/// and nine times between, both beams dark. Breaks when a beam's ramp is
/// moved into a dart, which would sweep the light across the canvas in
/// 0.1 s.
#[test]
fn no_beam_is_lit_during_a_dart() {
    let plan = Plan::build();
    let darts = plan.darts();
    assert_eq!(darts.len(), 9, "nine darts");
    for (t0, t1) in darts {
        for k in 0..=10 {
            let t = t0 + (t1 - t0) * f64::from(k) / 10.0;
            let p = plan.pose(t);
            assert_eq!((p.beam, p.block_beam), (0.0, 0.0), "a beam at {t}");
        }
    }
}

/// **The depth flips only mid-dart**: every change between in front and
/// behind falls inside a dart, at [`FLIP`] of it, or at the start of the
/// dart back in from the right while the saucer is off the canvas -- never
/// in a hover, so it never pops while the saucer is still; and it is only
/// ever in front or behind. Breaks when a flip is laid outside its dart.
#[test]
fn the_depth_flips_only_mid_dart() {
    let plan = Plan::build();
    let darts = plan.darts();
    let jumps = plan.depth.jumps();
    assert_eq!(jumps.len(), 8, "{jumps:?}");
    for t in jumps {
        let (t0, _) = darts
            .iter()
            .copied()
            .find(|&(t0, t1)| (t0..t1).contains(&t))
            .unwrap_or_else(|| panic!("the depth flips at {t}, in no dart"));
        let at = (t - t0) / DART_S;
        let off_canvas = plan.pose(t).cx > 840.0;
        assert!(
            (at - FLIP).abs() < 1e-9 || (at == 0.0 && off_canvas),
            "the depth flips {at} of the way through the dart at {t0}"
        );
    }
    for f in 0..FRAMES {
        let d = plan.pose(frame_t(f)).depth;
        assert!(d == FRONT || d == BEHIND, "frame {f}: depth {d}");
    }
}

/// **A beam is lit only from in front**: at every millisecond of the loop,
/// a lit beam, onto a letter or onto the block, comes from a saucer in
/// front of the letters, since from behind a counter it would fall below
/// the word. Breaks when a beam is turned on behind the word.
#[test]
fn a_beam_is_lit_only_from_in_front() {
    let plan = Plan::build();
    let mut lit = 0;
    for ms in 0..=10_000 {
        let p = plan.pose(f64::from(ms) / 1000.0);
        if p.beam > 0.0 || p.block_beam > 0.0 {
            lit += 1;
            assert_eq!(p.depth, FRONT, "a beam from behind at {ms} ms");
        }
    }
    assert!(lit > 2000, "{lit} ms lit");
}

/// **The planner's moves are the move table**, in order: each start,
/// what, duration, target and easing. Breaks when the choreography and the
/// table that documents it part.
#[test]
fn the_planner_is_the_move_table() {
    let moves = Plan::build().moves();
    assert_eq!(moves.len(), MOVES.len());
    for (m, (start, what, dur, target, easing)) in moves.iter().zip(MOVES) {
        let row = format!(
            "{:.3} {} {:.3} {} {}",
            m.start, m.what, m.dur, m.target, m.easing
        );
        let want = format!("{start:.3} {what} {dur:.3} {target} {easing}");
        assert_eq!(row, want);
    }
}

/// **Every pose is the designed one**: the 320 rows `ufo-poses.tsv` holds,
/// written by the planner the film was designed with, each at its frame's
/// time and printed to the same places, are this planner's to the last
/// place -- the centre to a thousandth of a pixel, the bank to a
/// hundred-thousandth of a radian, the depth to a tenth. Breaks on any
/// change to a move, an easing, the idle layer, the depth or the order the
/// segments are laid in.
#[test]
fn every_pose_is_the_designed_one() {
    let plan = Plan::build();
    let rows: Vec<&str> = include_str!("ufo-poses.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect();
    assert_eq!(rows.len(), FRAMES);
    for (f, row) in rows.iter().enumerate() {
        let t = frame_t(f);
        let p = plan.pose(t);
        let ours = format!(
            "{f} {t:.5} {:.3} {:.3} {:.5} {:.4} {:.4} {:.4} {:.5} {:.1}",
            p.cx,
            p.cy,
            p.bank * (std::f64::consts::PI / 180.0),
            p.beam,
            p.block_beam,
            p.lift,
            p.rim,
            p.depth
        );
        assert_eq!(&ours, row, "frame {f}");
    }
}

/// **The film's lines are the same bytes every time**, a line a frame on
/// the plasma film's grid, each frame's grid time and its planner time no
/// more than 9 ms apart, ending at the loop's 1000 hundredths. Breaks when
/// the planner reads anything but its table, or the lines leave the grid.
#[test]
fn the_lines_are_deterministic_and_on_the_grid() {
    let a = ufo::lines(&Plan::build(), 587.5);
    assert_eq!(a, ufo::lines(&Plan::build(), 587.5));
    assert!(a.starts_with("U 56.0000\n"));
    assert!(a.ends_with("E 1000\n"));
    let frames: Vec<Vec<&str>> = a
        .lines()
        .filter(|l| l.starts_with("F "))
        .map(|l| l.split(' ').collect())
        .collect();
    assert_eq!(frames.len(), FRAMES);
    for (f, v) in frames.iter().enumerate() {
        assert_eq!(v.len(), 14, "frame {f}: {v:?}");
        let cs: f64 = v[2].parse().expect("hundredths");
        let t: f64 = v[13].parse().expect("seconds");
        assert!(
            (cs / 100.0 - t).abs() < 0.009,
            "frame {f}: {cs} cs at {t} s"
        );
    }
}
