//! **The UFO logo's flight**: a curious alien saucer threading the
//! lockup's letters, as a move table baked once into one pose a frame. It
//! hovers behind the word in the gap between the `a` and the `u`, tilts
//! toward where it is going and is then there at once: behind the `a` into
//! the `b`'s counter, out in front over the `b` to light it, behind into
//! the `a`'s counter, out in front in the `u`'s opening, behind into the
//! `d`'s counter, then out onto the block, which it beams up; it streaks off
//! left behind the word, comes back in front from the right, sets the block
//! down and dips behind the word, home. `baud-app`'s `ufo_film.rs` draws
//! each line ([`lines`]) with the ufo trail's own body and beam; this module
//! decides what they do, as [`crate::plasma`] decides what the plasma does.
//!
//! **Curious, and alien.** Attention is aimed by a tilt, held in a hover,
//! re-aimed by small shifts, then acted on: the one trait a faceless object
//! shows by its position alone (Takayama, Dooley and Ju, HRI 2011; O'Rourke
//! et al., PLoS ONE 2010). What makes it alien rather than alive is that a
//! living character winds up before a move and overshoots after it
//! (Lasseter, SIGGRAPH 1987), and this one never does: it is still, then
//! elsewhere. Its route avoids symmetry -- left, up, down-right, up-right,
//! down-right, up-right, then left, each leg its own length -- as a search
//! by darts does.
//!
//! **In front of the letters and behind them**, as the plasma is: a depth
//! of +24 in front and -24 behind, drawn as the plasma's head is, `front =
//! clamp(0.5 + depth / 6, 0, 1)`, the body hidden behind the letters' ink
//! by `1 - front` and the beam reaching them from either side. The depth
//! flips at 0.35 of a dart, mid-streak, never in a hover, so it never pops
//! while the saucer is still (a design choice); a beam is lit only from in
//! front, above a letter, since from behind it would fall below the word
//! (a design choice).
//!
//! **Units**: canvas pixels on the film's 840 by 580 (the lockup's units
//! are `px / 2 - 60` across and `px / 2 - 75` down), seconds, and bank in
//! degrees, positive leaning right, into a move to the right. The film is
//! [`FRAMES`] frames over [`LOOP`] seconds, frame `f` posed at `f * LOOP /
//! FRAMES` and shown from its time on the plasma film's grid
//! ([`plasma::film_times_cs`]).
//!
//! **Each number says where it comes from**: a published source, named; a
//! rule of motion design, stated; a design choice, tuned on renders of the
//! film; or a place measured on the bare logo. The arithmetic calls none of
//! the platform's `sin` ([`det`]), so the poses are the same bits on every
//! machine.
//!
//! **The seam**: the loop ends where it began, level, behind the word, both
//! beams off, the block down, the rim lights a whole number of turns round,
//! and every period of the idle layer divides the loop. `tests/ufo.rs`
//! holds each, that no beam is lit during a dart, that the depth flips only
//! inside a dart, and that a beam is lit only in front.

use crate::json::Value;
use crate::logo::{self, Fail, Files, Mp4};
use crate::{det, plasma, py};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

/// The loop, in seconds, and its frames: the plasma film's ten seconds and
/// 320 frames, so the block blinks the default's twenty whole cycles of
/// eight frames a phase across it.
pub const LOOP: f64 = 10.0;
pub const FRAMES: usize = 320;

/// **The saucer's cell**, in canvas pixels: 28 of the lockup's units at the
/// film's two pixels a unit, the size the trail's saucer is drawn at over a
/// cell that tall, its beam's rings 0.22 of it apart. About 62 by 22 pixels
/// of saucer, so it sits behind a counter rather than inside it. A design
/// choice.
pub const C: f64 = 56.0;

/// The cursor block on the canvas, left, top, right, bottom, as the lockup
/// files place it at the film's scale.
pub const BLOCK: (f64, f64, f64, f64) = (567.0, 236.0, 608.0, 327.0);

/// **In front of the letters and behind them**: the plasma's depths, and
/// the span the film draws the change over, `front = clamp(0.5 + depth /
/// DEPTH_SPAN, 0, 1)`, as the plasma's head is drawn. A design choice.
pub const FRONT: f64 = 24.0;
pub const BEHIND: f64 = -24.0;
pub const DEPTH_SPAN: f64 = 6.0;
/// Where in a dart the depth flips: 0.35 of the way, mid-streak. A design
/// choice.
pub const FLIP: f64 = 0.35;

/// **Where it stops**, each measured on the bare logo, the dark half with
/// the block shown, 840 by 580: home, behind the word in the gap between
/// the `a` and the `u` (360, 288), the saucer showing through the gap and
/// the `u`'s opening; behind the `b`'s counter (259, 292); behind the
/// `a`'s (326, 304); inside the `u`'s opening, in front (396, 276); behind
/// the `d`'s counter (465, 292). The ink spans x 231 to 607 and y 235 to
/// 327; the letters run `b` 231-284, `a` 297-352, `u` 369-421, `d`
/// 437-490.
pub const HOME: (f64, f64) = (360.0, 288.0);
pub const C_B: (f64, f64) = (259.0, 292.0);
pub const C_A: (f64, f64) = (326.0, 304.0);
pub const C_D: (f64, f64) = (465.0, 292.0);
pub const U_OPEN: (f64, f64) = (396.0, 276.0);
/// Over the `b`, in front, its beam on the letter (a design choice); and
/// over the block, a cell above its top.
pub const OVER_B: (f64, f64) = (258.0, 172.0);
pub const OVER_BLOCK: (f64, f64) = ((BLOCK.0 + BLOCK.2) / 2.0, BLOCK.1 - 1.0 * C);

/// **The hover's bob**: 5 pixels each way on the canvas, 2.5 on the
/// README's 420-pixel width, over 2.5 s. A hover bobs 2 to 3 pixels on a
/// period of about 2.5 s; the pixels are doubled for the canvas, which is
/// shown at half its size (a design choice).
pub const BOB_PX: f64 = 5.0;
pub const BOB_S: f64 = 2.5;
/// **The rim's wobble**: 2 degrees each way over 5 s, the idle sway of a
/// hovering craft, 1 to 2 degrees on about 5 s.
pub const WOBBLE_DEG: f64 = 2.0;
pub const WOBBLE_S: f64 = 5.0;
/// **The rim lights**: 0.8 turns a second, a turn every 1.25 s, a slow
/// idle chase; and 2 turns more, eased in from the abduction, excited (a
/// design choice).
pub const RIM_HZ: f64 = 0.8;
pub const RIM_EXTRA: f64 = 2.0;
/// **A dart**: 0.10 s, whatever the distance. A machine's dart takes 80 to
/// 120 ms, with no anticipation and no overshoot.
pub const DART_S: f64 = 0.10;
/// A light's ramp on or off: 0.18 s, linear, never during a dart, so no
/// change of light is quicker than 0.15 s. A design choice, checked by the
/// flash census.
pub const RAMP_S: f64 = 0.18;

/// **A tilt's time**: 140 ms and 10 ms a degree -- the turn law the
/// motion's rules give, a tilt under 10 degrees going unseen (Sirithunge et
/// al., arXiv 2026 preprint), so none here is smaller.
#[must_use]
pub fn turn_s(deg: f64) -> f64 {
    0.14 + 0.010 * deg.abs()
}

/// **Minimum jerk**, `10u^3 - 15u^4 + 6u^5` (Flash and Hogan, J Neurosci
/// 1985): tilts, peers, the wobble and the hop; never a dart.
#[must_use]
pub fn minjerk(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    u * u * u * (10.0 - 15.0 * u + 6.0 * u * u)
}

/// **A dart's easing**, `1 - (1 - u)^3`: full speed at once and a hard
/// stop, arriving dead -- no wind-up and no overshoot.
#[must_use]
pub fn dart(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    let v = 1.0 - u;
    1.0 - v * v * v
}

/// A straight ramp.
#[must_use]
pub fn linear(u: f64) -> f64 {
    u.clamp(0.0, 1.0)
}

/// Smoothstep, the lift's and the set-down's easing, as the caret's lift.
#[must_use]
pub fn smooth(u: f64) -> f64 {
    let t = u.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// An easing.
pub type Ease = fn(f64) -> f64;

/// A segment of a channel: from `t0` to `t1`, from `a` to `b`, eased.
#[derive(Clone, Copy)]
struct Seg {
    t0: f64,
    t1: f64,
    a: f64,
    b: f64,
    ease: Ease,
}

/// **One channel**: its segments in the order they were added, holding the
/// last value between them.
#[derive(Clone)]
pub struct Track {
    v0: f64,
    segs: Vec<Seg>,
}

impl Track {
    fn new(v0: f64) -> Self {
        Self {
            v0,
            segs: Vec::new(),
        }
    }

    /// A segment from the last one's end, at `t0` for `dur`, to `b`; its end.
    fn to(&mut self, t0: f64, dur: f64, b: f64, ease: Ease) -> f64 {
        let a = self.at_end();
        self.segs.push(Seg {
            t0,
            t1: t0 + dur,
            a,
            b,
            ease,
        });
        t0 + dur
    }

    /// The value the last segment ends at.
    fn at_end(&self) -> f64 {
        self.segs.last().map_or(self.v0, |s| s.b)
    }

    /// **The value at `t`**: each segment in the order added until one
    /// starts after `t`, the last one reached deciding -- its end once
    /// past it, which a segment of no length always is.
    #[must_use]
    pub fn at(&self, t: f64) -> f64 {
        let mut v = self.v0;
        for s in &self.segs {
            if t < s.t0 {
                break;
            }
            v = if t >= s.t1 {
                s.b
            } else {
                s.a + (s.b - s.a) * (s.ease)((t - s.t0) / (s.t1 - s.t0))
            };
        }
        v
    }

    /// The times its value jumps: each segment of no length.
    #[must_use]
    pub fn jumps(&self) -> Vec<f64> {
        let mut v = self.v0;
        let mut out = Vec::new();
        for s in &self.segs {
            if s.t1 == s.t0 && s.b != v {
                out.push(s.t0);
            }
            v = s.b;
        }
        out
    }
}

/// **A row of the move table**: when it starts, what it is, how long it
/// takes, its target and its easing.
#[derive(Clone, Debug, PartialEq)]
pub struct Move {
    pub start: f64,
    pub what: &'static str,
    pub dur: f64,
    pub target: String,
    pub easing: &'static str,
}

/// The darts' easing, as the table names it.
const HARD: &str = "hard out, no ease in, no overshoot";

/// **The move table**, start and duration in seconds, in the order the
/// moves start. [`Plan::moves`] is the planner's own, and `tests/ufo.rs`
/// holds the two equal.
pub const MOVES: [(f64, &str, f64, &str, &str); 48] = [
    (0.000, "hover", 0.450, "", ""),
    (0.450, "tilt", 0.290, "-15 deg", "minimum-jerk"),
    (0.740, "dart", 0.100, "(259, 292)", HARD),
    (0.840, "level", 0.290, "0 deg", "minimum-jerk"),
    (0.840, "hover", 0.280, "", ""),
    (1.120, "peer", 0.160, "x +12 px", "minimum-jerk"),
    (1.280, "hover", 0.220, "", ""),
    (1.500, "peer", 0.180, "x -20 px", "minimum-jerk"),
    (1.680, "hover", 0.160, "", ""),
    (1.840, "tilt", 0.260, "+12 deg", "minimum-jerk"),
    (2.100, "dart", 0.100, "(258, 172)", HARD),
    (2.200, "level", 0.260, "0 deg", "minimum-jerk"),
    (2.200, "beam on", 0.180, "the b", "linear"),
    (2.200, "hover", 0.460, "", ""),
    (2.660, "tilt", 0.300, "+16 deg", "minimum-jerk"),
    (2.960, "hover", 0.300, "", ""),
    (3.020, "beam off", 0.180, "", "linear"),
    (3.260, "dart", 0.100, "(326, 304)", HARD),
    (3.360, "hover", 0.220, "", ""),
    (3.580, "dart", 0.100, "(396, 276)", HARD),
    (3.680, "hover", 0.180, "", ""),
    (3.860, "dart", 0.100, "(465, 292)", HARD),
    (3.960, "hover", 0.260, "", ""),
    (4.220, "dart", 0.100, "(588, 180)", HARD),
    (4.320, "level", 0.300, "0 deg", "minimum-jerk"),
    (4.620, "freeze", 0.100, "", ""),
    (4.620, "hover", 0.120, "", ""),
    (4.740, "rim speeds up", 1.800, "+2 turns", "minimum-jerk"),
    (4.740, "beam on", 0.180, "the block", "linear"),
    (4.920, "hover", 0.120, "", ""),
    (
        5.040,
        "lift",
        0.750,
        "the block into the saucer",
        "smoothstep",
    ),
    (5.790, "wobble", 0.440, "+12, -9, +5, 0 deg", "minimum-jerk"),
    (6.230, "hover", 0.100, "", ""),
    (6.330, "tilt", 0.300, "-16 deg", "minimum-jerk"),
    (6.630, "dart", 0.100, "(-84, 290)", HARD),
    (6.730, "hover", 0.300, "", ""),
    (7.030, "dart", 0.100, "(588, 180)", HARD),
    (
        7.130,
        "servo ring",
        0.480,
        "x -8, +3.2, -1.2, 0 px",
        "minimum-jerk steps",
    ),
    (7.130, "level", 0.300, "0 deg", "minimum-jerk"),
    (7.610, "beam on", 0.180, "the block", "linear"),
    (
        7.790,
        "set down",
        0.700,
        "the block in its place",
        "smoothstep",
    ),
    (8.490, "beam off", 0.180, "", "linear"),
    (8.490, "hover", 0.180, "", ""),
    (8.670, "hop", 0.350, "y -14 px and back", "minimum-jerk"),
    (9.020, "tilt", 0.290, "-15 deg", "minimum-jerk"),
    (9.310, "dart", 0.100, "(360, 288)", HARD),
    (9.410, "level", 0.290, "0 deg", "minimum-jerk"),
    (9.700, "hover", 0.300, "", ""),
];

/// **A pose**: the saucer's centre, its bank in degrees, the free beam's
/// and the block beam's light, the block's lift from 0 (in its place) to 1
/// (in the saucer), the rim lights' turns and its depth against the
/// letters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub cx: f64,
    pub cy: f64,
    pub bank: f64,
    pub beam: f64,
    pub block_beam: f64,
    pub lift: f64,
    pub rim: f64,
    pub depth: f64,
}

/// **The plan**: every channel, and the moves it was built from.
#[derive(Clone)]
pub struct Plan {
    pub x: Track,
    pub y: Track,
    pub bank: Track,
    pub depth: Track,
    pub beam: Track,
    pub block_beam: Track,
    pub lift: Track,
    pub damp: Track,
    pub excite: Track,
    /// The moves in the order the planner wrote them.
    pub written: Vec<Move>,
    /// When the last move ends.
    pub end: f64,
}

impl Plan {
    fn note(
        &mut self,
        start: f64,
        what: &'static str,
        dur: f64,
        target: String,
        easing: &'static str,
    ) {
        self.written.push(Move {
            start,
            what,
            dur,
            target,
            easing,
        });
    }

    fn hold(&mut self, t0: f64, dur: f64) -> f64 {
        self.note(t0, "hover", dur, String::new(), "");
        t0 + dur
    }

    fn tilt(&mut self, t0: f64, deg: f64) -> f64 {
        let d = turn_s(deg - self.bank.at_end());
        self.bank.to(t0, d, deg, minjerk);
        self.note(t0, "tilt", d, format!("{deg:+.0} deg"), "minimum-jerk");
        t0 + d
    }

    fn level(&mut self, t0: f64) -> f64 {
        let d = turn_s(self.bank.at_end());
        self.bank.to(t0, d, 0.0, minjerk);
        self.note(t0, "level", d, "0 deg".into(), "minimum-jerk");
        t0 + d
    }

    /// A dart to `p` at `depth`: the depth flipping at [`FLIP`] of it when
    /// it changes; the bob and the wobble damped to nothing over the 60 ms
    /// before it and back over the 120 ms after it (a design choice; a
    /// craft holds still for a dart).
    fn dart_to(&mut self, t0: f64, p: (f64, f64), depth: f64) -> f64 {
        if depth != self.depth.at_end() {
            self.depth.to(t0 + FLIP * DART_S, 0.0, depth, linear);
        }
        self.x.to(t0, DART_S, p.0, dart);
        self.y.to(t0, DART_S, p.1, dart);
        self.damp.to(t0 - 0.06, 0.06, 1.0, linear);
        self.damp.to(t0 + DART_S, 0.12, 0.0, linear);
        self.note(
            t0,
            "dart",
            DART_S,
            format!("({:.0}, {:.0})", p.0, p.1),
            HARD,
        );
        t0 + DART_S
    }

    /// **A peer**: a sideways slide with no turn, re-aiming attention by a
    /// small shift, as a hawk peers (O'Rourke et al., PLoS ONE 2010); here
    /// smaller than over open ground, to stay in the counter (a design
    /// choice).
    fn shift(&mut self, t0: f64, dx: f64, dur: f64) -> f64 {
        let to = self.x.at_end() + dx;
        self.x.to(t0, dur, to, minjerk);
        self.note(t0, "peer", dur, format!("x {dx:+.0} px"), "minimum-jerk");
        t0 + dur
    }

    /// **The plan, built**: the moves run in order.
    ///
    /// # Panics
    /// When the moves run past the loop.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "the choreography, one beat after another"
    )]
    pub fn build() -> Self {
        let mut p = Self {
            x: Track::new(HOME.0),
            y: Track::new(HOME.1),
            bank: Track::new(0.0),
            depth: Track::new(BEHIND),
            beam: Track::new(0.0),
            block_beam: Track::new(0.0),
            lift: Track::new(0.0),
            damp: Track::new(0.0),
            excite: Track::new(0.0),
            written: Vec::new(),
            end: 0.0,
        };
        let mut t = 0.0;
        // 1. Home: behind the word, peeking through the gap between the `a`
        // and the `u`. The holds are 0.10 to 0.46 s, set by hand to sum to
        // the loop; a hover's holds run log-normal with medians of 0.3 to
        // 2 s.
        t = p.hold(t, 0.45);
        // 2. Through the word: a tilt, the thought shown before the act; a
        // dart behind the `a` into the `b`'s counter; two peers there.
        t = p.tilt(t, -15.0);
        t = p.dart_to(t, C_B, BEHIND);
        p.level(t);
        t = p.hold(t, 0.28);
        t = p.shift(t, 12.0, 0.16);
        t = p.hold(t, 0.22);
        t = p.shift(t, -20.0, 0.18);
        t = p.hold(t, 0.16);
        // 3. Out in front over the `b`, and its beam lights the letter.
        t = p.tilt(t, 12.0);
        t = p.dart_to(t, OVER_B, FRONT);
        p.level(t);
        p.beam.to(t, RAMP_S, 1.0, linear);
        p.note(t, "beam on", RAMP_S, "the b".into(), "linear");
        t = p.hold(t, 0.46);
        // The cock: attention aimed right, at the block, held 0.30 s (a
        // design choice), and the tilt into the next dart.
        t = p.tilt(t, 16.0);
        t = p.hold(t, 0.30);
        p.beam.to(t - 0.24, RAMP_S, 0.0, linear);
        p.note(t - 0.24, "beam off", RAMP_S, String::new(), "linear");
        // 4. The zig-zag through the letters, with short hovers between (a
        // machine's search darts at sharp angles, hovering 150 to 300 ms):
        // behind into the `a`'s counter, out in the `u`'s opening, behind
        // into the `d`'s counter, then out onto the block.
        t = p.dart_to(t, C_A, BEHIND);
        t = p.hold(t, 0.22);
        t = p.dart_to(t, U_OPEN, FRONT);
        t = p.hold(t, 0.18);
        t = p.dart_to(t, C_D, BEHIND);
        t = p.hold(t, 0.26);
        t = p.dart_to(t, OVER_BLOCK, FRONT);
        t = p.level(t);
        // 5. The abduction: the held breath, the bob frozen over the
        // block; the beam; a held beat of 0.12 s (a beat before an act
        // holds 100 to 130 ms); the lift; the wobble.
        p.damp.to(t, 0.10, 1.0, linear);
        p.note(t, "freeze", 0.10, String::new(), "");
        t = p.hold(t, 0.12);
        p.excite.to(t, 1.8, 1.0, minjerk);
        p.note(
            t,
            "rim speeds up",
            1.8,
            format!("+{RIM_EXTRA:.0} turns"),
            "minimum-jerk",
        );
        t = p.block_beam.to(t, RAMP_S, 1.0, linear);
        p.note(t - RAMP_S, "beam on", RAMP_S, "the block".into(), "linear");
        t = p.hold(t, 0.12);
        // The lift, 0.75 s, smoothstep: the block rises toward the saucer,
        // shrinks to 0.3 and fades, as the caret's lift does (a design
        // choice).
        t = p.lift.to(t, 0.75, 1.0, smooth);
        p.note(
            t - 0.75,
            "lift",
            0.75,
            "the block into the saucer".into(),
            "smoothstep",
        );
        p.block_beam.to(t, RAMP_S, 0.0, linear);
        p.damp.to(t, 0.12, 0.0, linear);
        // The wobble of satisfaction: two decaying swings, each at least
        // 10 degrees so they show; the values a design choice.
        let w0 = t;
        for (k, deg) in [12.0, -9.0, 5.0, 0.0].into_iter().enumerate() {
            #[allow(clippy::cast_precision_loss, reason = "k in 0..4")]
            let k = k as f64;
            p.bank.to(w0 + 0.11 * k, 0.11, deg, minjerk);
        }
        p.note(
            w0,
            "wobble",
            0.44,
            "+12, -9, +5, 0 deg".into(),
            "minimum-jerk",
        );
        t = p.hold(w0 + 0.44, 0.10);
        // 6. Leave left behind the word at the letters' height, a streak
        // behind them, then gone 0.30 s (a design choice).
        t = p.tilt(t, -16.0);
        t = p.dart_to(t, (-1.5 * C, 290.0), BEHIND);
        t = p.hold(t, 0.30);
        // 7. Back in front from the right in one dart, ringing to a stop
        // once: the one machine stop, kept for the abduction; its steps a
        // design choice.
        p.x.to(t, 0.0, 840.0 + 1.5 * C, linear);
        p.y.to(t, 0.0, OVER_BLOCK.1 - 20.0, linear);
        p.depth.to(t, 0.0, FRONT, linear);
        p.bank.to(t, 0.0, -16.0, linear);
        t = p.dart_to(t, OVER_BLOCK, FRONT);
        let r0 = t;
        for (k, dx) in [-8.0, 3.2, -1.2, 0.0].into_iter().enumerate() {
            #[allow(clippy::cast_precision_loss, reason = "k in 0..4")]
            let k = k as f64;
            p.x.to(r0 + 0.12 * k, 0.12, OVER_BLOCK.0 + dx, minjerk);
        }
        p.note(
            r0,
            "servo ring",
            0.48,
            "x -8, +3.2, -1.2, 0 px".into(),
            "minimum-jerk steps",
        );
        p.level(r0);
        t = r0 + 0.48;
        // 8. Set it down, 0.70 s; a hop, up 14 px in 0.13 s and back in
        // 0.22 s -- a hawk's bob of about 8 px, its size a design choice;
        // and a dip back behind the word, home.
        t = p.block_beam.to(t, RAMP_S, 1.0, linear);
        p.note(t - RAMP_S, "beam on", RAMP_S, "the block".into(), "linear");
        t = p.lift.to(t, 0.70, 0.0, smooth);
        p.note(
            t - 0.70,
            "set down",
            0.70,
            "the block in its place".into(),
            "smoothstep",
        );
        p.block_beam.to(t, RAMP_S, 0.0, linear);
        p.note(t, "beam off", RAMP_S, String::new(), "linear");
        t = p.hold(t, 0.18);
        let up = p.y.at_end() - 14.0;
        p.y.to(t, 0.13, up, minjerk);
        p.y.to(t + 0.13, 0.22, OVER_BLOCK.1, minjerk);
        p.note(t, "hop", 0.35, "y -14 px and back".into(), "minimum-jerk");
        t += 0.35;
        t = p.tilt(t, -15.0);
        t = p.dart_to(t, HOME, BEHIND);
        t = p.level(t);
        assert!(t <= LOOP + 1e-9, "the moves run to {t:.3} s, past {LOOP}");
        p.end = t;
        p.note(t, "hover", LOOP - t, String::new(), "");
        p
    }

    /// The move table: the moves the planner wrote, by their start, those
    /// starting together in the order written.
    #[must_use]
    pub fn moves(&self) -> Vec<Move> {
        let mut m = self.written.clone();
        m.sort_by(|a, b| a.start.total_cmp(&b.start));
        m
    }

    /// **The pose at `t`**: the channels, with the idle layer -- the bob
    /// and the wobble, damped by the darts and the held breath -- and the
    /// rim lights' turns.
    #[must_use]
    pub fn pose(&self, t: f64) -> Pose {
        let d = self.damp.at(t);
        let ph = 2.0 * std::f64::consts::PI * t;
        let by = BOB_PX * det::sin(ph / BOB_S) * (1.0 - d);
        let wb = WOBBLE_DEG * det::sin(ph / WOBBLE_S) * (1.0 - d);
        Pose {
            cx: self.x.at(t),
            cy: self.y.at(t) + by,
            bank: self.bank.at(t) + wb,
            beam: self.beam.at(t),
            block_beam: self.block_beam.at(t),
            lift: self.lift.at(t),
            rim: RIM_HZ * t + RIM_EXTRA * self.excite.at(t),
            depth: self.depth.at(t),
        }
    }

    /// The darts, each its start and end.
    #[must_use]
    pub fn darts(&self) -> Vec<(f64, f64)> {
        self.written
            .iter()
            .filter(|m| m.what == "dart")
            .map(|m| (m.start, m.start + m.dur))
            .collect()
    }
}

/// Frame `f`'s time on the planner's clock, `f * LOOP / FRAMES` seconds.
#[must_use]
pub fn frame_t(f: usize) -> f64 {
    #[allow(clippy::cast_precision_loss, reason = "f under FRAMES")]
    let f = f as f64;
    f * LOOP / FRAMES as f64
}

/// **The lines the film reads**: `U` and the cell, then a frame a line,
/// `F i cs x y bank front free onto_block away lift lift_x rim t` -- its
/// start on the plasma film's grid in hundredths, the saucer's centre on
/// the canvas, its bank in radians, how much of it is in front of the
/// letters, the free beam's and the block beam's light, whether the block
/// is away from its place and how far it has risen, the rising block's
/// centre across (`block_x`), the rim lights' turns and the frame's time on
/// the planner's clock -- and `E` and the hundredths the loop wraps at.
///
/// # Panics
/// When the plasma's grid is not [`FRAMES`] frames.
#[must_use]
pub fn lines(plan: &Plan, block_x: f64) -> String {
    let grid = plasma::film_times_cs();
    assert_eq!(grid.len(), FRAMES, "a time on the grid for every frame");
    let mut s = format!("U {C:.4}\n");
    for (f, cs) in grid.iter().enumerate() {
        let t = frame_t(f);
        let p = plan.pose(t);
        writeln!(
            s,
            "F {f} {cs} {:.4} {:.4} {:.6} {:.4} {:.4} {:.4} {} {:.4} {:.4} {:.5} {t:.5}",
            p.cx,
            p.cy,
            p.bank * (std::f64::consts::PI / 180.0),
            (0.5 + p.depth / DEPTH_SPAN).clamp(0.0, 1.0),
            p.beam,
            p.block_beam,
            u8::from(p.lift > 0.0),
            p.lift,
            block_x,
            p.rim,
        )
        .expect("a String");
    }
    writeln!(s, "E {}", plasma::LOOP_CS).expect("a String");
    s
}

// ---------------------------------------------------------------- the run

/// The UFO logo's GIFs, each half's, in [`logo::HALVES`]' order.
pub const GIFS: [&str; 2] = [
    "docs/assets/demo/ufo-logo-dark.gif",
    "docs/assets/demo/ufo-logo-light.gif",
];

/// A GIF's provenance file: its path and this.
pub const PROVENANCE: &str = ".provenance.toml";

/// The tests that hold the files in the tree: WCAG 2.3.1's census of each,
/// and each one's saucer where the planner poses it.
const TESTS: [&[&str]; 2] = [
    &["-p", "baud-bench", "--test", "ufo_logo_flashes"],
    &[
        "-p",
        "baud",
        "--bin",
        "baud",
        "--",
        "ufo_film::tests::the_ufo_logos_fly_their_poses",
    ],
];

/// What `--ufo` reads the run by: the census of the GIF as it decodes.
#[derive(Debug)]
pub struct Census {
    /// The most flashes at a pixel in a second, general and red.
    pub general: String,
    pub red: String,
    /// The area flashing and the strict reading's, of the threshold.
    pub area: String,
    pub strict: String,
    pub threshold: String,
    /// The verdicts.
    pub outcome: String,
    pub strict_outcome: String,
}

/// **A GIF's provenance file**: every fact of how it is made that is the
/// same on every run, so `--check` holds it byte for byte as it holds the
/// GIF.
#[must_use]
pub fn provenance(
    gif_path: &str,
    gif: &[u8],
    flight: &str,
    frames: usize,
    total: u32,
    page: [u8; 3],
    c: &Census,
) -> String {
    let name = gif_path.rsplit('/').next().unwrap_or(gif_path);
    let mut s = String::new();
    let mut line = |k: &str, v: String| writeln!(s, "{k:<17}= {v}").expect("a String");
    let q = |v: &str| format!("\"{v}\"");
    line("file", q(name));
    line("sha256", q(&crate::sha256::hex(gif)));
    line("bytes", gif.len().to_string());
    line("frames", frames.to_string());
    line("size", format!("[{}, {}]", logo::W, logo::H));
    line("lasts_cs", total.to_string());
    line("repeats", q("for ever, a NETSCAPE2.0 block of count 0"));
    line(
        "page",
        q(&format!("#{:02X}{:02X}{:02X}", page[0], page[1], page[2])),
    );
    line("made_by", q("tools/baud-pictures logo --ufo"));
    line(
        "drawn_by",
        q(
            "crates/baud-app/src/ufo_film.rs: the lockup by baud's CPU renderer, the saucer and its beam by baud-render's ufo trail",
        ),
    );
    line(
        "flight",
        q("tools/baud-pictures/src/ufo.rs, its move table"),
    );
    line("flight_sha256", q(&crate::sha256::hex(flight.as_bytes())));
    line("walls", q(logo::WALLS));
    line("walls_sha256", q(logo::WALLS_SHA256));
    line("encoder", q(logo::GIFSKI));
    line("encoder_nixpkgs", q(logo::GIFSKI_NIXPKGS));
    line(
        "encoder_settings",
        format!(
            "[\"--fps\", \"50\", \"--quality\", \"{}\", \"--motion-quality\", \"{}\", \"--lossy-quality\", \"{}\", \"--fixed-color\", \"{:02X}{:02X}{:02X}\", \"--repeat\", \"0\"]",
            logo::QUALITY,
            logo::MOTION,
            logo::LOSSY,
            page[0],
            page[1],
            page[2]
        ),
    );
    line("snap_levels", logo::SNAP.to_string());
    line("zlib", q(crate::zlib::ZLIB));
    line("zlib_nixpkgs", q(logo::NIXPKGS_REV));
    s.push_str("\n[census]\n");
    let mut line = |k: &str, v: &str| writeln!(s, "{k:<17}= {v}").expect("a String");
    line("samples_a_second", "100");
    line("pixels_a_point", "2");
    line("fed", "\"twice over, the repeat's join counted\"");
    line("general_flashes", &c.general);
    line("red_flashes", &c.red);
    line("area", &c.area);
    line("strict", &c.strict);
    line("threshold", &c.threshold);
    line("outcome", &format!("\"{}\"", c.outcome));
    line("strict_outcome", &format!("\"{}\"", c.strict_outcome));
    format!(
        "# How {gif_path} is made: every line the same on every run.\n\
         # `baud-pictures logo --ufo --check` makes the GIF and this file again\n\
         # and compares both byte for byte.\n{s}"
    )
}

/// One half's GIF, its provenance, its flight, the lines that report it and
/// its MP4 under `--mp4`.
type UfoMade = (Vec<u8>, String, String, Vec<String>, Option<Vec<u8>>);

/// **One half of the UFO logo**, made in `tmp` and checked.
#[allow(clippy::too_many_lines, reason = "the run's steps, in their order")]
fn make(
    root: &Path,
    (gifski, ff): (&Path, Option<&Path>),
    (h, gif_path): (&logo::Half, &str),
    tmp: &Path,
    plan: &Plan,
    bare: &Value,
) -> Result<UfoMade, Fail> {
    let half = h.name;
    let d = tmp.join(half);
    let block: Vec<f64> = bare.get("block")?.nums()?;
    let [x0, _, x1, _] = block[..] else {
        return Err(format!("{half}: the block's box is {block:?}"));
    };
    let flight = lines(plan, (x0 + x1) * 0.5);
    let flight_file = d.join("flight.txt");
    std::fs::write(&flight_file, &flight).map_err(|e| format!("{}: {e}", flight_file.display()))?;
    logo::cargo_test(
        root,
        &["ufo_film::film_the_ufo_logo", "--", "--ignored", "--exact"],
        &[
            ("BAUD_LOGO_OUT", &d),
            ("BAUD_LOGO_HALF", Path::new(half)),
            ("BAUD_UFO_FLIGHT", &flight_file),
        ],
        "the UFO film",
    )?;
    let m = logo::json_file(&d.join("film.json"))?;
    logo::size_half(&m, half, "the UFO film")?;
    logo::frames_match(&m, half)?;
    let delays = logo::delays_of(&m)?;
    let frames = logo::snapped(&logo::read(&d.join("frames.rgb"))?, delays.len(), h.page)?;
    let mut still = logo::canvas(&d.join("still.rgb"))?;
    logo::snap(&mut still, h.page);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "`int(v)` of pixel coordinates"
    )]
    let b: Vec<usize> = m
        .get("block")?
        .nums()?
        .iter()
        .map(|&v| v as usize)
        .collect();
    let round = 2usize;
    let around = [
        b[0].saturating_sub(round),
        b[1].saturating_sub(round),
        b[2] + round,
        b[3] + round,
    ];
    let (wrap, busiest) = wrap_holds(half, &frames, around)?;
    let mut names = Vec::with_capacity(frames.len());
    for (i, f) in frames.iter().enumerate() {
        let p = d.join(format!("{i:05}.png"));
        std::fs::write(&p, logo::quick_png(f)).map_err(|e| format!("{}: {e}", p.display()))?;
        names.push(p);
    }
    let b = logo::encode(gifski, &d, &delays, &names, h)?;
    if logo::encode(gifski, &d, &delays, &names, h)? != b {
        return Err(format!(
            "{half}: gifski wrote different bytes from the same frames"
        ));
    }
    let (got, loops, size) = logo::decoded(root, &b, &d)?;
    let total: u32 = delays.iter().sum();
    let lasts: u32 = got.iter().map(|(_, cs)| cs).sum();
    logo::gif_holds(half, lasts, total, loops, size, b.len())?;
    let movie = match ff {
        Some(ff) => {
            let m = logo::mp4(ff, &d, &frames, &delays, "ufo.mp4")?;
            if logo::mp4(ff, &d, &frames, &delays, "again.mp4")? != m {
                return Err(format!(
                    "{half}: ffmpeg wrote different MP4s from the same frames"
                ));
            }
            let said = logo::mp4_holds(ff, half, &d.join("ufo.mp4"), total)?;
            println!(
                "{half}: the UFO MP4, {} bytes, twice the same, reads {said}",
                m.len()
            );
            Some(m)
        }
        None => None,
    };
    let (share, worst_levels) = logo::haze(&frames, &got, h.page, &delays);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "`int(v)` of pixel coordinates"
    )]
    let word: Vec<usize> = m.get("word")?.nums()?.iter().map(|&v| v as usize).collect();
    let all: Vec<&[u8]> = got.iter().map(|(f, _)| f.as_slice()).collect();
    let (worst, median) = logo::contrast(
        &all,
        &still,
        [word[0], word[1], word[2], word[3]],
        h.page,
        h.word,
    );
    logo::contrast_holds(half, worst)?;
    // The GIF's own frames and delays: gifski merges a run of equal frames,
    // the canvas while the saucer is away off it, into one.
    let decoded: Vec<Vec<u8>> = got.iter().map(|(f, _)| f.clone()).collect();
    let decoded_cs: Vec<u32> = got.iter().map(|(_, cs)| *cs).collect();
    let c = logo::census(root, &decoded, &decoded_cs, &d, "gif")?;
    let census = Census {
        general: c.get("general_flashes")?.py(),
        red: c.get("red_flashes")?.py(),
        area: c.get("area")?.py(),
        strict: c.get("strict")?.py(),
        threshold: c.get("threshold")?.py(),
        outcome: c.get("outcome")?.text()?.to_string(),
        strict_outcome: c.get("strict_outcome")?.text()?.to_string(),
    };
    let lines = vec![
        format!(
            "{gif_path}: {} bytes, {} frames on #{:02X}{:02X}{:02X}, repeating every {total} cs",
            b.len(),
            got.len(),
            h.page[0],
            h.page[1],
            h.page[2]
        ),
        format!(
            "  the saucer: off the block, the wrap moves {wrap} pixels and the busiest step {busiest}"
        ),
        format!(
            "  haze: {}% of the page's pixels off it in the GIF, by {worst_levels} levels at worst",
            py::repr_float(share)
        ),
        format!(
            "  the word's contrast against the ground round it: worst {} to 1, median {}",
            py::repr_float(worst),
            py::repr_float(median)
        ),
        format!(
            "  the GIF's census: general {}, red {} flashes at a pixel in a second; area {}, strict {} of {}: {}, strict {}",
            census.general,
            census.red,
            census.area,
            census.strict,
            census.threshold,
            census.outcome,
            census.strict_outcome
        ),
    ];
    if census.outcome != "passes" || census.strict_outcome != "passes" {
        for l in &lines {
            println!("{l}");
        }
        return Err(format!(
            "{half}: the GIF's census fails: {}",
            logo::repr(&c)
        ));
    }
    let prov = provenance(gif_path, &b, &flight, got.len(), total, h.page, &census);
    Ok((b, prov, flight, lines, movie))
}

/// **The UFO's wrap is no jump**: the step from the last frame to the
/// first moves no more pixels off the block (`around`) than the film's
/// busiest step. Unlike the plasma's, a frame may be the bare logo: while
/// the saucer is off the canvas nothing moves, which is the beat. The
/// wrap's and the busiest step's.
///
/// # Errors
/// When the wrap jumps.
pub fn wrap_holds(
    half: &str,
    frames: &[Vec<u8>],
    around: [usize; 4],
) -> Result<(usize, usize), Fail> {
    let busiest = frames
        .windows(2)
        .map(|w| logo::moved(&w[1], &w[0], around))
        .max()
        .unwrap_or(0);
    let wrap = logo::moved(&frames[0], &frames[frames.len() - 1], around);
    if wrap > busiest {
        return Err(format!(
            "{half}: the wrap moves {wrap} pixels, past the film's busiest step's {busiest}"
        ));
    }
    Ok((wrap, busiest))
}

/// The tests that hold the files, in the tree or in `out`, their output
/// shown.
fn tests(root: &Path, out: Option<&Path>) -> Result<(), Fail> {
    for t in TESTS {
        let mut cmd = Command::new("cargo");
        cmd.args(["test", "--quiet", "--locked"])
            .args(logo::FAST)
            .args(t);
        if !t.contains(&"--") {
            cmd.arg("--");
        }
        cmd.args(["--nocapture", "--test-threads", "1"])
            .current_dir(root);
        if let Some(d) = out {
            cmd.env("BAUD_UFO_FILES", d);
        }
        let out = cmd
            .output()
            .map_err(|e| format!("cargo test did not run: {e}"))?;
        let text = String::from_utf8_lossy(&out.stderr).to_string()
            + &String::from_utf8_lossy(&out.stdout);
        for line in py::splitlines(&text) {
            if line.starts_with("ufo-logo-") || line.starts_with("test ") {
                println!("{line}");
            }
        }
        if !out.status.success() {
            println!("{text}");
            return Err(format!("the test {} fails", t.join(" ")));
        }
    }
    Ok(())
}

/// **`logo --ufo [--check | --out DIR] [--mp4]`**: the run, its lines
/// printed; the files that differ.
///
/// # Errors
/// What stops the run before the files are compared or written.
pub fn main(
    root: &Path,
    files: &Files,
    keep: Option<&Path>,
    movie: Mp4,
) -> Result<Vec<String>, Fail> {
    if movie == Mp4::Yes && !matches!(files, Files::Out(_)) {
        return Err(
            "--mp4 needs --out DIR: the MP4s are not committed, and left in the \
             tree they make the demo's --record refuse it as dirty"
                .into(),
        );
    }
    logo::libraries()?;
    let out = match files {
        Files::Out(d) => {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
            Some(
                d.canonicalize()
                    .map_err(|e| format!("{}: {e}", d.display()))?,
            )
        }
        _ => None,
    };
    let gifski = logo::gifski()?;
    let ff = match movie {
        Mp4::Yes => Some(logo::ffmpeg()?),
        Mp4::No => None,
    };
    let tmp = keep.map_or_else(
        || std::env::temp_dir().join(format!("baud-pictures-ufo-{}", std::process::id())),
        Path::to_path_buf,
    );
    if keep.is_none() {
        let _ = std::fs::remove_dir_all(&tmp);
    } else if std::fs::read_dir(&tmp).is_ok_and(|mut d| d.next().is_some()) {
        return Err(format!("{} is not empty", tmp.display()));
    }
    let result: Result<Vec<String>, Fail> = (|| {
        let (mut today, mut bare) = (Vec::new(), Vec::new());
        for h in &logo::HALVES {
            let d = tmp.join(h.name);
            std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
            let (t, m) = logo::bare(root, h.name, &d)?;
            today.push(t);
            bare.push(m);
        }
        // The places it stops were measured on the bare logo: the lockup
        // must still be the drawing the walls hold.
        logo::walls_hold(root, &today[0], &tmp)?;
        let plan = Plan::build();
        let (mut bad, mut flights) = (Vec::new(), Vec::new());
        for ((h, gif_path), bare) in logo::HALVES.iter().zip(GIFS).zip(&bare) {
            let (g, prov, flight, lines, movie) = make(
                root,
                (&gifski, ff.as_deref()),
                (h, gif_path),
                &tmp,
                &plan,
                bare,
            )?;
            flights.push(flight);
            if let Some(m) = movie {
                let name = Path::new(gif_path).with_extension("mp4");
                let name = name.file_name().ok_or("a file's name")?;
                let to = out.as_ref().ok_or("--mp4 without --out")?.join(name);
                std::fs::write(&to, &m).map_err(|e| format!("{}: {e}", to.display()))?;
                println!("wrote {}", to.display());
            }
            for line in lines {
                println!("{line}");
            }
            let prov_path = format!("{gif_path}{PROVENANCE}");
            for (path, b) in [(gif_path.to_string(), g), (prov_path, prov.into_bytes())] {
                let at = root.join(&path);
                if *files == Files::Check {
                    let committed = std::fs::read(&at).ok();
                    let (line, refusal) = logo::compared(&path, committed.as_deref(), &b);
                    println!("{line}");
                    bad.extend(refusal);
                } else {
                    let name = Path::new(&path).file_name().ok_or("a file's name")?;
                    let to = out.as_ref().map_or(at, |d| d.join(name));
                    std::fs::write(&to, &b).map_err(|e| format!("{}: {e}", to.display()))?;
                    println!("wrote {}", to.display());
                }
            }
        }
        if flights[0] != flights[1] {
            return Err("the two halves fly different paths".into());
        }
        Ok(bad)
    })();
    if keep.is_none() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    let bad = result?;
    tests(root, out.as_deref())?;
    Ok(bad)
}

/// Which logo `logo` films.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    /// The README's plasma logo.
    Plasma,
    /// The UFO logo.
    Ufo,
}

/// **`--ufo` taken out of `logo`'s arguments**, wherever it stands, and the
/// rest.
#[must_use]
pub fn asked(argv: &[String]) -> (Which, Vec<String>) {
    let rest: Vec<String> = argv.iter().filter(|a| *a != "--ufo").cloned().collect();
    match rest.len() == argv.len() {
        true => (Which::Plasma, rest),
        false => (Which::Ufo, rest),
    }
}
