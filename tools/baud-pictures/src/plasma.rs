//! **The README logo's plasma: the frames' primitives, for the logo film.**
//!
//! `baud-pictures plasma <geometry dir> <dark|light> <on_ms> <off_ms> <out>`
//!
//! The plasma moves round and through the lockup `baud ❯█` as if each
//! glyph were a solid wall: in front of the letters or behind them,
//! changing side only through an opening -- the counters of b, a and d,
//! the slots between the letters, the u's open top, over, under or round
//! the ends -- hidden behind a letter but for what shows through its
//! holes, and against a stroke squashed flat and run along it. A swirling
//! current carries it, and a short tail of wisps slides round the letters.
//!
//! **The head** follows a chain of moves drawn from a seeded random number
//! ([`Rng`]), never a lap: a sweep to a side, a turn inside the word, a
//! drop through a counter, a weave round a letter through the slots either
//! side, a graze along a stroke and a hit against one. Each move is timed,
//! laid as a Hermite spline through its waypoints, and kept only if the
//! head stays in the envelope, never comes behind the block and never sits
//! in the letters' plane inside a stroke (`validate`); a director steers
//! the mix toward the time behind, the time in contact and the counter
//! passes the targets ask, and the last move closes the chain on its
//! start, so the chain is a loop of `LOOP` seconds. The head's depth is
//! `ZMAX` in front or behind and passes through the plane only at an
//! opening; within `CONTACT` of it the walls act (`front_of`, `squeezed`,
//! `push`). A hit squashes the head against the stroke and throws two
//! tongues along its contour.
//!
//! **The current** is Bridson's curl noise: the curl of a stream function
//! of twelve waves, each periodic in the loop, its potential ramped to
//! zero at the slabs' 3D distance so the flow runs along a stroke and
//! never into it (`flow`). It carries the head off the chain, held still
//! round each crossing and contact (`displacement`), and three strands of
//! parcels shed from the head, which settle onto the slabs' face and slide
//! along a stroke they meet; the links between successive parcels are the
//! tail (`parcel_rope`), each living 0.30 s, longer hugging a stroke.
//! Three surges a loop brighten it. A faint residual flickers on two
//! iso-lines round the letters, brighter where the head passes
//! (`residual`).
//!
//! **What it writes**: the README film's frames, each `F <i> <cs>`, its
//! time from the film's start in hundredths, then a line a plasma strand,
//! `C front light bx by fx fy width unit age phase fine k hz ions seed` in
//! lockup units: its weight in front of the glyphs (the rest behind), its
//! light, the chord from `b` to `f`, its width and plasma cell, its body's
//! age and its detail clock (`effect::Live`), its filaments' fine wiggle
//! (amplitude, cycles a cell and a second), its ions a cell, and its seed.
//! A last line `E <cs>` gives the time the film wraps at. The film
//! (`baud-app/src/logo_film.rs`) draws each through the renderer's splash
//! layers. On stdout, one JSON line: the seed, the loop's length, its events
//! and frames, and the chain's stats.
//!
//! **The film is the whole loop**: the chain's `LOOP` seconds, twenty of the
//! default blink's 500 ms cycles, which `on_ms` and `off_ms` name and the
//! frame grid is built on (the frame grid is 30 ms, with 40 ms closing each
//! 250 ms phase: at 20 ms and 30 the whole loop's GIF is 6.4 MB, past the 5
//! MB limit), at full light from its first frame to its last, so the
//! file repeats with no bare logo and nothing to join: the frame after the
//! last is the first, the chain, the current, the surges and the events each
//! being periodic in the loop. [`Plasma::first_unrepeated`] holds that each
//! frame's strands equal those a whole loop later, which tests the paths that
//! read raw time. The current reads time already reduced to the loop, so this
//! test cannot see a current that does not close; the strand digests pin the
//! waves, and the decoded film's wrap step is measured against every other step.
//!
//! **[`Plasma::strands`] keeps the 4.5 s the film was before**, the stretch
//! with the most of the chain's events between two fades, starting on a
//! blink cycle (`choose_window`): its digests are the ones the Python wrote,
//! so it is what holds this port to that program.
//!
//! Lockup units (u); the canvas is px = (u + 60) * 2, py = (v + 75) * 2,
//! and the geometry is [`crate::geometry`]'s. All arithmetic is IEEE's own
//! or [`crate::det`]'s, so the frames are the same on every platform.
//!
//! **Ported from the Python and NumPy program that wrote them, operation
//! for operation**, so the strands are its bytes: each expression keeps its
//! grouping, and where Python or NumPy is not Rust the port says whose it
//! is -- [`py::sum`]'s compensated builtin `sum`, [`py::fsum`],
//! [`py::rem`]'s float `%`, [`py::max`] and [`py::min`]'s first-wins ties,
//! round half to even, and [`numpy::round_to`], which NumPy's own `float64`
//! scalars took where a waypoint came off a contour walk.

use crate::numpy;
use crate::py;
use crate::{det, geometry};
use std::fmt::Write as _;
use std::path::Path;

const LOOP: f64 = 10.0;
/// `LOOP` in hundredths, the film's length.
const LOOP_CS: i64 = 1000;
/// A chord every 12.5 ms.
const STEP: f64 = 0.0125;
const ZMAX: f64 = 24.0;
/// Slab thickness, u.
const TAU: f64 = 6.0;
const CONTACT: f64 = 7.0;
/// The head's chord across, and its plasma cell, u.
const P_WIDTH: f64 = 20.0;
const P_UNIT: f64 = 30.0;
const HW: f64 = P_WIDTH / 2.0;
/// The head's centre off the stroke while grazing, u.
const GRAZE_C: f64 = 8.0;
/// The block as the walls have it, where the path was chosen.
///
/// The lockup's now stands 2.26 units higher, which
/// [`crate::logo::MARK_MOVED`] holds. The chain stays out from behind it by the margins here, over 4.5 units;
/// a strand's own ends are not kept out, and a few fall on the block.
const BLOCK: (f64, f64, f64, f64) = (223.5, 45.0, 244.0, 91.0);
const ENVELOPE: (f64, f64, f64, f64) = (8.0, 18.0, 290.0, 116.0);
const W_: usize = geometry::W;
const H_: usize = geometry::H;

/// `NCH = int(round(LOOP / STEP))`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "800"
)]
fn nch() -> usize {
    py::round(LOOP / STEP) as usize
}

/// `int(x)` of a float: toward zero.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the generator's small numbers"
)]
fn int(x: f64) -> i64 {
    x as i64
}

/// An index Python's `int(x)` names, `x` never negative.
#[allow(clippy::cast_sign_loss, reason = "never negative")]
fn idx(x: f64) -> usize {
    let i = int(x);
    assert!(i >= 0, "an index from a negative float");
    i as usize
}

/// An exact small integer as a float, as Python's int meets a float.
#[allow(clippy::cast_precision_loss, reason = "small integers, each exact")]
fn fl(i: i64) -> f64 {
    i as f64
}

/// Python's `max(a, b, c)`: the first of the greatest.
fn max3(a: f64, b: f64, c: f64) -> f64 {
    py::max(py::max(a, b), c)
}

fn h01(n: u64) -> f64 {
    let mut x = ((n.wrapping_mul(0x9E37_79B9)) & 0xFFFF_FFFF) ^ 0x85EB_CA6B;
    x ^= x >> 16;
    x = (x.wrapping_mul(0x7FEB_352D)) & 0xFFFF_FFFF;
    x ^= x >> 15;
    x = (x.wrapping_mul(0x846C_A68B)) & 0xFFFF_FFFF;
    x ^= x >> 16;
    #[allow(clippy::cast_precision_loss, reason = "24 bits")]
    let v = (x >> 8) as f64;
    v / 16_777_216.0
}

/// The seeded random number: a hash of the salt and a counter.
pub struct Rng {
    salt: u64,
    i: u64,
}

impl Rng {
    /// The stream `salt` names.
    #[must_use]
    pub fn new(salt: u64) -> Rng {
        Rng { salt, i: 0 }
    }

    /// The next number in `[lo, hi)`.
    pub fn r(&mut self, lo: f64, hi: f64) -> f64 {
        self.i += 1;
        let n =
            (self.salt.wrapping_mul(7919)).wrapping_add(self.i.wrapping_mul(104_729)) & 0xFFFF_FFFF;
        lo + (hi - lo) * h01(n)
    }

    /// The next number in `[0, 1)`.
    pub fn u(&mut self) -> f64 {
        self.r(0.0, 1.0)
    }

    /// A name drawn by its weight.
    pub fn pick(&mut self, weights: &[(&'static str, f64)]) -> &'static str {
        let x = self.u();
        let mut acc = 0.0;
        let tot = py::sum(weights.iter().map(|w| w.1)).unwrap_or(0.0);
        for &(name, w) in weights {
            acc += w / tot;
            if x < acc {
                return name;
            }
        }
        weights[weights.len() - 1].0
    }
}

fn smooth(e0: f64, e1: f64, x: f64) -> f64 {
    let k = py::min(py::max((x - e0) / (e1 - e0), 0.0), 1.0);
    k * k * (3.0 - 2.0 * k)
}

fn off_block(x: f64, y: f64) -> f64 {
    let dx = max3(BLOCK.0 - x, x - BLOCK.2, 0.0);
    let dy = max3(BLOCK.1 - y, y - BLOCK.3, 0.0);
    det::hypot(dx, dy)
}

fn ease(x: f64) -> f64 {
    let x = py::min(py::max(x, 0.0), 1.0);
    let y = 1.0 - x;
    1.0 - y * y
}

fn inside(p: (f64, f64)) -> bool {
    let m = 4.0;
    ENVELOPE.0 + m <= p.0 && p.0 <= ENVELOPE.2 - m && ENVELOPE.1 + m <= p.1 && p.1 <= ENVELOPE.3 - m
}

/// `math.radians`: a product by `pi / 180`.
fn radians(x: f64) -> f64 {
    x * (std::f64::consts::PI / 180.0)
}

// ---- the openings -------------------------------------------------------

const HOLE: [(&str, (f64, f64)); 3] = [
    ("b", (69.25, 69.0)),
    ("a", (103.6, 77.0)),
    ("d", (171.75, 69.5)),
];
const NOTCH: (f64, f64) = (137.8, 62.0);
const SLOT: [(&str, f64, f64); 4] = [
    ("ba", 86.0, 72.0),
    ("au", 120.5, 72.0),
    ("ud", 154.75, 72.0),
    ("d>", 196.75, 68.0),
];
const SLOW: f64 = 80.0;
const SLOTV: f64 = 170.0;
const WALL_IN: f64 = 230.0;
const HOLD: f64 = 0.14;
const SLIDE_V: f64 = 170.0;

fn hole(nm: &str) -> Option<(f64, f64)> {
    HOLE.iter().find(|h| h.0 == nm).map(|h| h.1)
}

fn slot(nm: &str) -> Option<(f64, f64)> {
    SLOT.iter().find(|s| s.0 == nm).map(|s| (s.1, s.2))
}

fn weave(g: &str) -> (&'static str, &'static str) {
    match g {
        "a" => ("ba", "au"),
        "u" => ("au", "ud"),
        _ => ("ud", "d>"),
    }
}

fn ramp_of(nm: &str) -> f64 {
    if hole(nm).is_some() {
        0.15
    } else if slot(nm).is_some() {
        0.12
    } else if nm == "u" {
        0.14
    } else if nm == "endL" || nm == "endR" {
        0.22
    } else {
        0.18
    }
}

fn portal_x(nm: &str) -> f64 {
    if let Some(h) = hole(nm) {
        return h.0;
    }
    if let Some(s) = slot(nm) {
        return s.0;
    }
    match nm {
        "u" => NOTCH.0,
        "top" | "bot" => 140.0,
        "endL" => 36.0,
        "endR" => 266.0,
        _ => unreachable!("no portal {nm}"),
    }
}

const PORTAL_W: [(&str, f64); 12] = [
    ("b", 0.17),
    ("a", 0.09),
    ("d", 0.17),
    ("u", 0.07),
    ("ba", 0.07),
    ("au", 0.08),
    ("ud", 0.08),
    ("d>", 0.10),
    ("top", 0.05),
    ("bot", 0.05),
    ("endL", 0.05),
    ("endR", 0.02),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Start,
    Slow,
    Cross,
    SlotIn,
    SlotOut,
    XFree,
    Free,
    ContactIn,
    Graze,
    GrazeEnd,
    Wall,
    Hold,
    Slide,
    SlideEnd,
}

#[derive(Clone, Copy, Debug)]
enum Tag {
    None,
    Portal(&'static str),
    Graze(f64),
    Wall(f64, f64),
}

#[derive(Clone, Copy, Debug)]
struct Way {
    x: f64,
    y: f64,
    kind: Kind,
    tag: Tag,
}

fn way(x: f64, y: f64, kind: Kind, tag: Tag) -> Way {
    Way { x, y, kind, tag }
}

fn free(x: f64, y: f64) -> Way {
    way(x, y, Kind::Free, Tag::None)
}

#[derive(Clone)]
struct WallHit {
    t: f64,
    p: (f64, f64),
    n: (f64, f64),
    t_hold: f64,
    t_slide: f64,
}

#[derive(Clone)]
struct GrazeRec {
    walk: Vec<(f64, f64)>,
    sg: f64,
    t0: f64,
}

struct Move {
    kind: &'static str,
    dur: f64,
    x0: f64,
    x1: f64,
}

#[derive(Clone, Copy)]
struct Key {
    t: f64,
    x: f64,
    y: f64,
    v: Option<(f64, f64)>,
}

#[derive(Default)]
struct Chain {
    keys: Vec<Key>,
    cross: Vec<(f64, f64, &'static str)>,
    contact: Vec<(f64, f64)>,
    walls: Vec<WallHit>,
    grazes: Vec<GrazeRec>,
    squeeze: Vec<(f64, f64)>,
    moves: Vec<Move>,
    cyclic: bool,
    s0: i64,
    t: Vec<f64>,
    p: Vec<(f64, f64)>,
    v: Vec<(f64, f64)>,
    cr: Vec<(f64, f64, &'static str)>,
    cr_t: Vec<f64>,
}

/// `bisect.bisect_right`.
fn bisect_right(a: &[f64], x: f64) -> usize {
    a.partition_point(|&v| v <= x)
}

impl Chain {
    #[allow(clippy::cast_possible_wrap, reason = "a few dozen keys")]
    fn prepare(&mut self) {
        let n = self.keys.len() as i64;
        let t: Vec<f64> = self.keys.iter().map(|k| k.t).collect();
        let p: Vec<(f64, f64)> = self.keys.iter().map(|k| (k.x, k.y)).collect();
        let cyc = self.cyclic;
        #[allow(clippy::cast_sign_loss, reason = "rem_euclid is never negative")]
        let tp = |i: i64| -> (f64, (f64, f64)) {
            if cyc {
                let m = i.rem_euclid(n) as usize;
                (t[m] + LOOP * fl(i.div_euclid(n)), p[m])
            } else {
                let i = i.clamp(0, n - 1) as usize;
                (t[i], p[i])
            }
        };
        let mut v = Vec::new();
        for i in 0..n {
            #[allow(clippy::cast_sign_loss, reason = "i counts up from 0")]
            if let Some(k) = self.keys[i as usize].v {
                v.push(k);
                continue;
            }
            let ((ta, pa), (tb, pb), (tc, pc)) = (tp(i - 1), tp(i), tp(i + 1));
            if tb - ta <= 1e-9 && tc - tb <= 1e-9 {
                v.push((0.0, 0.0));
                continue;
            }
            if tb - ta <= 1e-9 {
                v.push(((pc.0 - pb.0) / (tc - tb), (pc.1 - pb.1) / (tc - tb)));
                continue;
            }
            if tc - tb <= 1e-9 {
                v.push(((pb.0 - pa.0) / (tb - ta), (pb.1 - pa.1) / (tb - ta)));
                continue;
            }
            let v1 = ((pb.0 - pa.0) / (tb - ta), (pb.1 - pa.1) / (tb - ta));
            let v2 = ((pc.0 - pb.0) / (tc - tb), (pc.1 - pb.1) / (tc - tb));
            let (wa, wb) = (tc - tb, tb - ta);
            v.push((
                (v1.0 * wa + v2.0 * wb) / (wa + wb),
                (v1.1 * wa + v2.1 * wb) / (wa + wb),
            ));
        }
        self.t = t;
        self.p = p;
        self.v = v;
        let mut cr = self.cross.clone();
        cr.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .expect("no NaN")
                .then(a.1.partial_cmp(&b.1).expect("no NaN"))
                .then(a.2.cmp(b.2))
        });
        self.cr_t = cr.iter().map(|c| c.0).collect();
        self.cr = cr;
    }

    fn xy(&self, t: f64) -> (f64, f64) {
        let (tt, p, v) = (&self.t, &self.p, &self.v);
        let n = tt.len();
        let (i, j, ta, tb, t) = if self.cyclic {
            let t = py::rem(t, LOOP);
            let b = bisect_right(tt, t);
            let (i, ta) = if b == 0 {
                (n - 1, tt[n - 1] - LOOP)
            } else {
                (b - 1, tt[b - 1])
            };
            let j = (i + 1) % n;
            let tb = tt[j] + if j <= i { LOOP } else { 0.0 };
            (i, j, ta, tb, t)
        } else {
            let b = bisect_right(tt, t);
            if b == 0 {
                return p[0];
            }
            let i = b - 1;
            if i >= n - 1 {
                return p[n - 1];
            }
            (i, i + 1, tt[i], tt[i + 1], t)
        };
        let h = tb - ta;
        let s = if h > 0.0 { (t - ta) / h } else { 0.0 };
        let ((x0, y0), (x1, y1)) = (p[i], p[j]);
        let ((vx0, vy0), (vx1, vy1)) = (v[i], v[j]);
        let s2 = s * s;
        let s3 = s2 * s;
        let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
        let h10 = s3 - 2.0 * s2 + s;
        let h01_ = -2.0 * s3 + 3.0 * s2;
        let h11 = s3 - s2;
        (
            h00 * x0 + h10 * h * vx0 + h01_ * x1 + h11 * h * vx1,
            h00 * y0 + h10 * h * vy0 + h01_ * y1 + h11 * h * vy1,
        )
    }

    fn z(&self, t: f64) -> f64 {
        let cr = &self.cr;
        let t = if self.cyclic { py::rem(t, LOOP) } else { t };
        let k = bisect_right(&self.cr_t, t);
        let s = if k.is_multiple_of(2) {
            self.s0
        } else {
            -self.s0
        };
        let a = if cr.is_empty() {
            1.0
        } else {
            let (prev, nxt) = if self.cyclic {
                let last = cr[cr.len() - 1];
                (
                    if k > 0 {
                        (cr[k - 1].0, cr[k - 1].1)
                    } else {
                        (last.0 - LOOP, last.1)
                    },
                    if k < cr.len() {
                        (cr[k].0, cr[k].1)
                    } else {
                        (cr[0].0 + LOOP, cr[0].1)
                    },
                )
            } else {
                (
                    if k > 0 {
                        (cr[k - 1].0, cr[k - 1].1)
                    } else {
                        (-10.0, 0.1)
                    },
                    if k < cr.len() {
                        (cr[k].0, cr[k].1)
                    } else {
                        (t + 10.0, 0.1)
                    },
                )
            };
            py::min(ease((t - prev.0) / prev.1), ease((nxt.0 - t) / nxt.1))
        };
        let mut z = fl(s) * ZMAX * a;
        let b = self.contact_w(t);
        if b > 0.0 {
            z *= 1.0 - (1.0 - 3.0 / ZMAX) * b;
        }
        z
    }

    fn contact_w(&self, t: f64) -> f64 {
        let mut best = 0.0;
        let offs: &[f64] = if self.cyclic {
            &[0.0, LOOP, -LOOP]
        } else {
            &[0.0]
        };
        for &(c0, c1) in &self.contact {
            for &off in offs {
                let tt = t + off;
                if c0 - 0.08 <= tt && tt <= c1 + 0.10 {
                    best = py::max(
                        best,
                        smooth(c0 - 0.08, c0, tt) * (1.0 - smooth(c1, c1 + 0.10, tt)),
                    );
                }
            }
        }
        best
    }

    fn in_contact(&self, t: f64, pad: f64) -> bool {
        self.contact.iter().any(|&(c0, c1)| {
            [0.0, LOOP, -LOOP]
                .iter()
                .any(|&off| c0 - pad <= t + off && t + off <= c1 + pad)
        })
    }

    fn in_squeeze(&self, t: f64) -> bool {
        self.squeeze.iter().any(|&(a, b)| {
            [0.0, LOOP, -LOOP]
                .iter()
                .any(|&off| a - 0.02 <= t + off && t + off <= b + 0.02)
        })
    }
}

/// What a move reads and leaves: where the head is, whether its x is a
/// NumPy `float64` (a contour walk's), the last height each side, the
/// openings just taken, and the grazes laid.
struct State {
    p: (f64, f64),
    p_np: bool,
    last_y: [Option<f64>; 2],
    recent: Vec<&'static str>,
    grazes: Vec<GrazeRec>,
}

type Got = Option<(Vec<Way>, Option<usize>)>;

/// One run's inputs: the walls' fields and the light of the half.
pub struct Plasma {
    sdf: Vec<f64>,
    nx: Vec<f64>,
    ny: Vec<f64>,
    holes: Vec<u8>,
    gain: f64,
    res_gain: f64,
    cpts: Vec<(f64, f64, f64, f64, &'static str)>,
    hpts: Vec<(f64, f64, f64, f64, f64)>,
}

/// The chain and everything built on it.
struct Built {
    ch: Chain,
    seed: u64,
    stats: String,
    surges: Vec<f64>,
    waves: Vec<(f64, f64, f64, f64, f64)>,
    flow_k: f64,
    disp: Vec<(f64, f64)>,
    tongues: Vec<Tongue>,
    res: Vec<Res>,
    parcels: Parcels,
}

struct Tongue {
    t: f64,
    life: f64,
    speed: f64,
    paths: [Vec<(f64, f64)>; 2],
    gain: f64,
    id: i64,
}

struct Res {
    p: (f64, f64),
    t: (f64, f64),
    ev: Vec<(f64, f64, f64, f64, f64)>,
    id: i64,
}

struct Parcels {
    x: Vec<Vec<f64>>,
    y: Vec<Vec<f64>>,
    z: Vec<Vec<f64>>,
    hug: Vec<Vec<f64>>,
}

const MIX: [(&str, f64); 6] = [
    ("sweep", 0.15),
    ("turn", 0.10),
    ("drop", 0.14),
    ("weave", 0.12),
    ("graze", 0.35),
    ("hit", 0.15),
];
const SEED: u64 = 2241;

/// Why a run stops: a refusal Python raised as `SystemExit` (caught where
/// the seed search catches it), or an assertion, which nothing catches.
#[derive(Debug)]
pub enum Stop {
    /// `SystemExit(msg)`.
    Exit(String),
    /// `AssertionError`.
    Assert(String),
}

/// `f"{x:.4f}"` of each float, then the seed.
fn fmt(v: &[f64; 14], seed: i64) -> String {
    let mut s = String::from("C");
    for x in v {
        write!(s, " {x:.4}").expect("a String");
    }
    write!(s, " {seed}").expect("a String");
    s
}

impl Plasma {
    /// The fields of the four files, the `f32`s widened.
    ///
    /// # Errors
    /// A field missing or of the wrong size, in Python's words.
    pub fn load(geom: &Path, dark: bool) -> Result<Plasma, Stop> {
        let field = |name: &str| -> Result<Vec<u8>, Stop> {
            let b = std::fs::read(geom.join(name))
                .map_err(|e| Stop::Exit(format!("plasma: {name}: {e}")))?;
            Ok(b)
        };
        let floats = |name: &str| -> Result<Vec<f64>, Stop> {
            let b = field(name)?;
            if b.len() / 4 != W_ * H_ {
                return Err(Stop::Exit(format!(
                    "plasma: {name} holds {} values, not {}",
                    b.len() / 4,
                    W_ * H_
                )));
            }
            Ok(b.chunks_exact(4)
                .map(|c| f64::from(f32::from_le_bytes(c.try_into().expect("four bytes"))))
                .collect())
        };
        let (sdf, nx, ny) = (floats("sdf.f32")?, floats("nx.f32")?, floats("ny.f32")?);
        let holes = field("holes.u8")?;
        if holes.len() != W_ * H_ {
            return Err(Stop::Exit(format!(
                "plasma: holes.u8 holds {} values, not {}",
                holes.len(),
                W_ * H_
            )));
        }
        Ok(Plasma::new(geometry::Fields { sdf, nx, ny, holes }, dark))
    }

    /// From fields already in hand.
    #[must_use]
    pub fn new(f: geometry::Fields, dark: bool) -> Plasma {
        // The fields as the plasma reads them: `f32` then widened.
        #[allow(clippy::cast_possible_truncation, reason = "the files are f32")]
        let narrow =
            |v: Vec<f64>| -> Vec<f64> { v.into_iter().map(|x| f64::from(x as f32)).collect() };
        let mut p = Plasma {
            sdf: narrow(f.sdf),
            nx: narrow(f.nx),
            ny: narrow(f.ny),
            holes: f.holes,
            gain: if dark { 0.80 } else { 0.78 },
            res_gain: if dark { 0.20 } else { 0.14 },
            cpts: Vec::new(),
            hpts: Vec::new(),
        };
        p.cpts = p.contact_points();
        p.hpts = p.hit_points();
        p
    }

    fn at(f: &[f64], j: usize, i: usize) -> f64 {
        f[j * W_ + i]
    }

    /// Field `f` (canvas px) at lockup (u, v), bilinear.
    fn bil(f: &[f64], u: f64, v: f64) -> f64 {
        let x = (u + 60.0) * 2.0;
        let y = (v + 75.0) * 2.0;
        let x = py::min(py::max(x, 0.0), 840.0 - 1.001);
        let y = py::min(py::max(y, 0.0), 580.0 - 1.001);
        let (i, j) = (idx(x), idx(y));
        let (fx, fy) = (x - fl(int(x)), y - fl(int(y)));
        Self::at(f, j, i) * (1.0 - fx) * (1.0 - fy)
            + Self::at(f, j, i + 1) * fx * (1.0 - fy)
            + Self::at(f, j + 1, i) * (1.0 - fx) * fy
            + Self::at(f, j + 1, i + 1) * fx * fy
    }

    fn sdf(&self, u: f64, v: f64) -> f64 {
        Self::bil(&self.sdf, u, v)
    }

    fn sdf_u(&self, u: f64, v: f64) -> f64 {
        self.sdf(u, v) / 2.0
    }

    fn normal(&self, u: f64, v: f64) -> (f64, f64) {
        let (nx, ny) = (Self::bil(&self.nx, u, v), Self::bil(&self.ny, u, v));
        let l = det::hypot(nx, ny);
        let l = if l == 0.0 { 1.0 } else { l };
        (nx / l, ny / l)
    }

    fn in_hole(&self, u: f64, v: f64) -> bool {
        let x = py::round((u + 60.0) * 2.0);
        let y = py::round((v + 75.0) * 2.0);
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_wrap,
            reason = "bounds checked first"
        )]
        let hit = 0 <= x
            && x < W_ as i64
            && 0 <= y
            && y < H_ as i64
            && self.holes[y as usize * W_ + x as usize] > 0;
        hit
    }

    /// The NumPy form of `bil` over the signed distance, `np.clip` bounds.
    fn sdf_field(&self, x: f64, y: f64) -> f64 {
        let px = numpy::clip((x + 60.0) * 2.0, 0.0, 840.0 - 1.001);
        let py_ = numpy::clip((y + 75.0) * 2.0, 0.0, 580.0 - 1.001);
        let (i, j) = (idx(px), idx(py_));
        let (fx, fy) = (px - fl(int(px)), py_ - fl(int(py_)));
        let f = &self.sdf;
        Self::at(f, j, i) * (1.0 - fx) * (1.0 - fy)
            + Self::at(f, j, i + 1) * fx * (1.0 - fy)
            + Self::at(f, j + 1, i) * (1.0 - fx) * fy
            + Self::at(f, j + 1, i + 1) * fx * fy
    }

    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "clipped to the canvas"
    )]
    fn nrm_field(&self, x: f64, y: f64) -> (f64, f64) {
        let px = int(((x + 60.0) * 2.0).round_ties_even()).clamp(0, W_ as i64 - 1) as usize;
        let py_ = int(((y + 75.0) * 2.0).round_ties_even()).clamp(0, H_ as i64 - 1) as usize;
        (self.nx[py_ * W_ + px], self.ny[py_ * W_ + px])
    }

    /// Walk the iso-line sdf = `iso_px` from `p`, `sgn` along the tangent,
    /// `dist` u.
    fn contour_walk(
        &self,
        p: (f64, f64),
        sgn: f64,
        dist: f64,
        iso_px: f64,
        step: f64,
    ) -> Vec<(f64, f64)> {
        let mut pts = vec![p];
        let (mut x, mut y) = p;
        for _ in 0..int(dist / step) {
            let (nx, ny) = self.normal(x, y);
            let (tx, ty) = (-ny * sgn, nx * sgn);
            x += tx * step;
            y += ty * step;
            for _ in 0..3 {
                let (nx, ny) = self.normal(x, y);
                let e = (self.sdf(x, y) - iso_px) / 2.0;
                x -= nx * e;
                y -= ny * e;
            }
            pts.push((x, y));
        }
        pts
    }

    fn contact_points(&self) -> Vec<(f64, f64, f64, f64, &'static str)> {
        let mut out = Vec::new();
        let iso = GRAZE_C * 2.0;
        let ds = numpy::arange(1.0, 14.0, 1.0);
        for v in numpy::arange(34.0, 100.0, 1.5) {
            for u in numpy::arange(40.0, 232.0, 1.5) {
                let s = self.sdf(u, v);
                if (s - iso).abs() > 0.8 || self.in_hole(u, v) {
                    continue;
                }
                let (nx, ny) = self.normal(u, v);
                if off_block(u, v) < HW + 7.0 {
                    continue;
                }
                if !ds
                    .iter()
                    .all(|&d| self.sdf(u + nx * d, v + ny * d) > iso - 0.6)
                {
                    continue;
                }
                let g = glyph_at(u - nx * GRAZE_C, v - ny * GRAZE_C);
                if g == "#" || g == "?" {
                    continue;
                }
                out.push((u, v, nx, ny, g));
            }
        }
        out
    }

    fn hit_points(&self) -> Vec<(f64, f64, f64, f64, f64)> {
        let mut out = Vec::new();
        for &(u, v, nx, ny, _) in &self.cpts {
            if py::max(nx.abs(), ny.abs()) < 0.93 {
                continue;
            }
            let ray = if nx.abs() > 0.9 { 12.0 } else { 24.0 };
            if numpy::arange(1.0, ray, 1.0)
                .iter()
                .all(|&d| self.sdf(u + nx * d, v + ny * d) > GRAZE_C * 2.0 - 0.5)
            {
                out.push((u, v, nx, ny, ray));
            }
        }
        out
    }
}

/// A box, x0 y0 x1 y1.
type Box4 = (f64, f64, f64, f64);

const GLYPH_BOX: [(&str, Box4); 6] = [
    ("b", (55.0, 42.5, 83.5, 88.5)),
    ("a", (88.5, 53.0, 117.5, 88.5)),
    ("u", (123.5, 54.0, 151.5, 88.5)),
    ("d", (158.0, 42.5, 186.0, 88.5)),
    (">", (207.5, 55.0, 218.0, 80.5)),
    ("#", (223.5, 45.0, 244.0, 91.0)),
];

fn glyph_at(u: f64, v: f64) -> &'static str {
    let (mut best, mut bd) = ("?", 1e9);
    for &(g, (x0, y0, x1, y1)) in &GLYPH_BOX {
        let dx = max3(x0 - u, u - x1, 0.0);
        let dy = max3(y0 - v, v - y1, 0.0);
        let d = det::hypot(dx, dy);
        if d < bd {
            best = g;
            bd = d;
        }
    }
    if bd <= 8.0 { best } else { "?" }
}

// ---- the waypoints ------------------------------------------------------

fn hole_way(nm: &'static str, r: &mut Rng, vertical: bool, dirx: f64, down: bool) -> Vec<Way> {
    let (cx, cy) = hole(nm).expect("a hole");
    let (slow, cross) = (
        |x, y| way(x, y, Kind::Slow, Tag::Portal(nm)),
        |x, y| way(x, y, Kind::Cross, Tag::Portal(nm)),
    );
    if vertical {
        let s = if down { 1.0 } else { -1.0 };
        let jx = r.r(-1.5, 1.5);
        return vec![
            slow(cx + jx, cy - 30.0 * s),
            slow(cx + jx, cy - 4.0 * s),
            cross(cx, cy),
            slow(cx - jx, cy + 4.0 * s),
            slow(cx - jx, cy + 30.0 * s),
        ];
    }
    let vs = if r.u() < 0.5 { 1.0 } else { -1.0 };
    let ang_in = vs * radians(r.r(40.0, 68.0));
    let ang_out = ang_in + radians(r.r(-14.0, 14.0));
    let din = (dirx * det::cos(ang_in), det::sin(ang_in));
    let dout = (dirx * det::cos(ang_out), det::sin(ang_out));
    vec![
        slow(cx - din.0 * 24.0, cy - din.1 * 24.0),
        slow(cx - din.0 * 3.0, cy - din.1 * 3.0),
        cross(cx, cy),
        slow(cx + dout.0 * 3.0, cy + dout.1 * 3.0),
        slow(cx + dout.0 * 26.0, cy + dout.1 * 26.0),
    ]
}

fn slot_way(nm: &'static str, down: bool) -> Vec<Way> {
    let (cx, cy) = slot(nm).expect("a slot");
    let (ya, yb) = if down { (36.0, 100.0) } else { (100.0, 36.0) };
    vec![
        way(cx, ya, Kind::SlotIn, Tag::Portal(nm)),
        way(cx, cy, Kind::Cross, Tag::Portal(nm)),
        way(cx, yb, Kind::SlotOut, Tag::Portal(nm)),
    ]
}

fn notch_way(r: &mut Rng, down: bool) -> Vec<Way> {
    let (cx, cy) = NOTCH;
    let u = Tag::Portal("u");
    if down {
        let a = way(cx + r.r(-2.0, 2.0), 34.0, Kind::Slow, u);
        let c = way(cx, cy, Kind::Cross, u);
        let bx = cx + r.r(-2.0, 2.0);
        let b = way(bx, 34.0 + r.r(0.0, 6.0), Kind::Slow, u);
        return vec![a, c, b];
    }
    let a = way(cx + r.r(-2.0, 2.0), 36.0, Kind::Slow, u);
    let c = way(cx, cy, Kind::Cross, u);
    let b = way(cx + r.r(-2.0, 2.0), 30.0, Kind::Slow, u);
    vec![a, c, b]
}

fn portal_way(nm: &'static str, r: &mut Rng, dirx: f64, y_now: f64) -> Vec<Way> {
    if let Some((_, hy)) = hole(nm) {
        if r.u() < 0.4 {
            return hole_way(nm, r, true, 1.0, y_now < hy);
        }
        return hole_way(nm, r, false, dirx, false);
    }
    if let Some((_, sy)) = slot(nm) {
        let down = if r.u() < 0.8 { y_now < sy } else { r.u() < 0.5 };
        return slot_way(nm, down);
    }
    let xf = |x, y, nm| way(x, y, Kind::XFree, Tag::Portal(nm));
    match nm {
        "u" => notch_way(r, true),
        "top" => vec![xf(r.r(75.0, 205.0), 31.0, "top")],
        "bot" => vec![xf(r.r(75.0, 205.0), 101.0, "bot")],
        "endL" => {
            let x = r.r(33.0, 40.0);
            vec![xf(x, r.r(46.0, 86.0), "endL")]
        }
        "endR" => {
            let x = r.r(262.0, 270.0);
            vec![xf(x, r.r(30.0, 104.0), "endR")]
        }
        _ => unreachable!("no portal {nm}"),
    }
}

fn side_point(r: &mut Rng, side: usize, last_y: &[Option<f64>; 2]) -> (f64, f64) {
    let mut y = 0.0;
    for _ in 0..20 {
        y = r.r(30.0, 104.0);
        match last_y[side] {
            None => break,
            Some(l) if (y - l).abs() >= 14.0 => break,
            Some(_) => {}
        }
    }
    let x = if side == 0 {
        r.r(16.0, 32.0)
    } else {
        r.r(258.0, 282.0)
    };
    (x, y)
}

fn pick_portals(
    r: &mut Rng,
    x_from: f64,
    x_to: f64,
    n: usize,
    avoid: &[&str],
) -> Vec<&'static str> {
    let (lo, hi) = (py::min(x_from, x_to), py::max(x_from, x_to));
    let mut cand: Vec<(&'static str, f64)> = PORTAL_W
        .iter()
        .copied()
        .filter(|&(nm, _)| {
            lo + 4.0 < portal_x(nm) && portal_x(nm) < hi - 4.0 && !avoid.contains(&nm)
        })
        .collect();
    let mut out = Vec::new();
    for _ in 0..n {
        if cand.is_empty() {
            break;
        }
        let nm = r.pick(&cand);
        out.push(nm);
        cand.retain(|&(a, _)| a != nm && (portal_x(a) - portal_x(nm)).abs() > 25.0);
    }
    let sign = if x_to > x_from { 1.0 } else { -1.0 };
    out.sort_by(|a, b| {
        (portal_x(a) * sign)
            .partial_cmp(&(portal_x(b) * sign))
            .expect("no NaN")
    });
    out
}

/// Python's `min(max(v, lo), hi)` with integer bounds.
fn clamp_py(v: f64, lo: f64, hi: f64) -> f64 {
    py::min(py::max(v, lo), hi)
}

impl Plasma {
    fn mv_sweep(r: &mut Rng, st: &State) -> Got {
        let (x, y) = st.p;
        let here = usize::from(x >= 147.0);
        let same = r.u() < 0.42;
        let side = if same { here } else { 1 - here };
        let (tx, ty) = side_point(r, side, &st.last_y);
        let mut w = Vec::new();
        if same {
            // in, a turn inside the word, and back out to the same side
            let xt = r.r(85.0, 205.0);
            let yt = clamp_py(y + r.r(-30.0, 30.0), 30.0, 104.0);
            let dirx = if xt > x { 1.0 } else { -1.0 };
            let n = usize::from(r.u() < 0.5);
            for nm in pick_portals(r, x, xt, n, &st.recent) {
                w.extend(portal_way(nm, r, dirx, y));
            }
            w.push(free(xt, yt));
            let n = usize::from(r.u() < 0.35);
            for nm in pick_portals(r, xt, tx, n, &st.recent) {
                w.extend(portal_way(nm, r, -dirx, yt));
            }
        } else {
            let dirx = if tx > x { 1.0 } else { -1.0 };
            let c = r.u();
            let n = if c < 0.4 {
                0
            } else if c < 0.85 {
                1
            } else {
                2
            };
            for nm in pick_portals(r, x, tx, n, &st.recent) {
                w.extend(portal_way(nm, r, dirx, y));
            }
            if n == 0 {
                let a = r.r(80.0, 200.0);
                w.push(free(
                    a,
                    clamp_py(y + (ty - y) * 0.5 + r.r(-22.0, 22.0), 30.0, 104.0),
                ));
            }
        }
        w.push(free(tx, ty));
        Some((w, Some(side)))
    }

    fn mv_turn(r: &mut Rng, st: &State) -> Got {
        let (x, y) = st.p;
        let xt = r.r(75.0, 210.0);
        if (xt - x).abs() < 30.0 {
            return None;
        }
        let yt = r.r(30.0, 104.0);
        let dirx = if xt > x { 1.0 } else { -1.0 };
        let mut w = Vec::new();
        if r.u() < 0.5 {
            for nm in pick_portals(r, x, xt, 1, &st.recent) {
                w.extend(portal_way(nm, r, dirx, y));
            }
        }
        w.push(free(xt, yt));
        let xe = xt - dirx * r.r(14.0, 40.0);
        let d = if r.u() < 0.5 {
            r.r(16.0, 34.0)
        } else {
            -r.r(16.0, 34.0)
        };
        let ye = clamp_py(yt + d, 28.0, 106.0);
        w.push(free(xe, ye));
        Some((w, None))
    }

    fn mv_drop(r: &mut Rng, st: &State) -> Got {
        let (_, y) = st.p;
        let nm = r.pick(&[("b", 0.32), ("a", 0.18), ("d", 0.32), ("u", 0.18)]);
        if st.recent.contains(&nm) {
            return None;
        }
        if nm == "u" {
            let cx = NOTCH.0;
            let a = cx + r.r(-14.0, 14.0);
            let mut w = vec![free(a, r.r(28.0, 32.0))];
            w.extend(notch_way(r, true));
            let a = cx + r.r(-30.0, 30.0);
            w.push(free(a, r.r(28.0, 40.0)));
            return Some((w, None));
        }
        let (cx, cy) = hole(nm).expect("a hole");
        let down = if r.u() < 0.75 { y < cy } else { r.u() < 0.5 };
        let hw = hole_way(nm, r, true, 1.0, down);
        let s = if down { 1.0 } else { -1.0 };
        let mut w = vec![free(cx + r.r(-10.0, 10.0), cy - 40.0 * s)];
        w.extend(hw);
        let a = cx + r.r(-24.0, 24.0);
        w.push(free(a, cy + r.r(38.0, 46.0) * s));
        Some((w, None))
    }

    fn mv_weave(r: &mut Rng, st: &State) -> Got {
        let (x, _) = st.p;
        let g = r.pick(&[("a", 0.34), ("u", 0.33), ("d", 0.33)]);
        let (mut g1, mut g2) = weave(g);
        let (s1, s2) = (slot(g1).expect("a slot"), slot(g2).expect("a slot"));
        if (s2.0 - x).abs() < (s1.0 - x).abs() {
            std::mem::swap(&mut g1, &mut g2);
        }
        if st.recent.contains(&g1) || st.recent.contains(&g2) {
            return None;
        }
        let (c1, y1) = slot(g1).expect("a slot");
        let (c2, y2) = slot(g2).expect("a slot");
        let top_first = r.u() < 0.6;
        let ya = if top_first { 36.0 } else { 100.0 };
        let s = if top_first { 1.0 } else { -1.0 };
        let gx = 0.5 * (c1 + c2);
        let ylow = y1 + 12.0 * s;
        let mut w = vec![
            way(c1, ya, Kind::SlotIn, Tag::Portal(g1)),
            way(c1, y1, Kind::Cross, Tag::Portal(g1)),
            free(c1, ylow),
        ];
        let a = gx + r.r(-3.0, 3.0);
        w.push(free(a, y1 + r.r(10.0, 16.0) * s));
        w.extend([
            free(c2, y2 + 12.0 * s),
            way(c2, y2, Kind::Cross, Tag::Portal(g2)),
            way(c2, ya, Kind::SlotOut, Tag::Portal(g2)),
        ]);
        let dirx = if c2 > c1 { 1.0 } else { -1.0 };
        let a = c2 + dirx * r.r(10.0, 34.0);
        w.push(free(a, ya + r.r(-6.0, 6.0) * s));
        Some((w, None))
    }

    #[allow(clippy::cast_precision_loss, reason = "a count of contact points")]
    fn mv_graze(&self, r: &mut Rng, st: &mut State) -> Got {
        let (x, y) = st.p;
        let cand: Vec<_> = self
            .cpts
            .iter()
            .filter(|c| {
                let d = det::hypot(c.0 - x, c.1 - y);
                20.0 < d && d < 150.0
            })
            .collect();
        if cand.is_empty() {
            return None;
        }
        let n = cand.len();
        let &(u, v, nx, ny, _) = cand[idx(r.u() * n as f64) % n];
        let gv = r.r(110.0, 170.0);
        let dur = r.r(0.35, 0.95);
        let sg = if r.u() < 0.5 { 1.0 } else { -1.0 };
        let walk = self.contour_walk((u, v), sg, gv * dur, GRAZE_C * 2.0, 1.0);
        let mut keep = vec![walk[0]];
        for &p in &walk[1..] {
            if off_block(p.0, p.1) < HW + 7.0
                || self.in_hole(p.0, p.1)
                || (self.sdf(p.0, p.1) - GRAZE_C * 2.0).abs() > 1.5
            {
                break;
            }
            keep.push(p);
        }
        if keep.len() < 20 {
            return None;
        }
        let walk = keep;
        let ax = u + nx * r.r(12.0, 18.0);
        let a = (ax, v + ny * r.r(12.0, 18.0));
        if !inside(a) {
            return None;
        }
        let mut w = vec![way(a.0, a.1, Kind::ContactIn, Tag::None)];
        for i in (6..walk.len()).step_by(6) {
            w.push(way(walk[i].0, walk[i].1, Kind::Graze, Tag::Graze(gv)));
        }
        if w[w.len() - 1].kind != Kind::Graze || w.len() < 3 {
            return None;
        }
        let (ex, ey) = (w[w.len() - 1].x, w[w.len() - 1].y);
        let (enx, eny) = self.normal(ex, ey);
        let (tx, ty) = (-eny * sg, enx * sg);
        let l = r.r(14.0, 30.0);
        let gx = ex + enx * l + tx * r.r(0.0, 14.0);
        w.push(way(
            gx,
            ey + eny * l + ty * r.r(0.0, 14.0),
            Kind::GrazeEnd,
            Tag::None,
        ));
        let last = w[w.len() - 1];
        if !inside((last.x, last.y)) {
            return None;
        }
        st.grazes.push(GrazeRec {
            walk,
            sg,
            t0: f64::NAN,
        });
        Some((w, None))
    }

    #[allow(clippy::cast_precision_loss, reason = "a count of hit points")]
    fn mv_hit(&self, r: &mut Rng, st: &State) -> Got {
        let (x, y) = st.p;
        let cand: Vec<_> = self
            .hpts
            .iter()
            .filter(|c| {
                let d = det::hypot(c.0 - x, c.1 - y);
                20.0 < d && d < 160.0
            })
            .collect();
        if cand.is_empty() {
            return None;
        }
        let n = cand.len();
        let &(u, v, nx, ny, ray) = cand[idx(r.u() * n as f64) % n];
        let a = (u + nx * ray, v + ny * ray);
        if !inside(a) {
            return None;
        }
        let (tx, ty) = (-ny, nx);
        let sg = if r.u() < 0.5 { 1.0 } else { -1.0 };
        let cont = self.contour_walk((u, v), sg, 0.2 * SLIDE_V, GRAZE_C * 2.0, 1.0);
        for &p in &cont {
            if off_block(p.0, p.1) < HW + 7.0 || self.in_hole(p.0, p.1) {
                return None;
            }
        }
        let mid = cont[cont.len() / 2];
        let end = cont[cont.len() - 1];
        let mut w = vec![
            way(a.0, a.1, Kind::ContactIn, Tag::None),
            way(u, v, Kind::Wall, Tag::Wall(nx, ny)),
            way(u, v, Kind::Hold, Tag::Wall(nx, ny)),
            way(mid.0, mid.1, Kind::Slide, Tag::None),
            way(end.0, end.1, Kind::SlideEnd, Tag::None),
        ];
        let (ex, ey) = end;
        let (enx, eny) = self.normal(ex, ey);
        let fx = ex + enx * r.r(16.0, 30.0) + tx * sg * r.r(0.0, 12.0);
        let fy = ey + eny * r.r(16.0, 30.0) + ty * sg * r.r(0.0, 12.0);
        w.push(free(fx, fy));
        if !inside((fx, fy)) {
            return None;
        }
        Some((w, None))
    }
}

/// Times for each waypoint after `start`; fixed-speed segments first, the
/// free ones share what is left. `None` if the speed is out of range.
fn time_way(
    w: &[Way],
    start: (f64, f64),
    t0: f64,
    dur: f64,
    lo: f64,
    speed: Option<f64>,
) -> Option<(Vec<(f64, Way)>, f64)> {
    let mut pts = vec![way(start.0, start.1, Kind::Start, Tag::None)];
    pts.extend_from_slice(w);
    let (mut fixed_t, mut free_len) = (0.0, 0.0);
    let mut segs = Vec::new();
    for pair in pts.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let l = det::hypot(b.x - a.x, b.y - a.y);
        let dt = if b.kind == Kind::Hold {
            Some(HOLD)
        } else if matches!(b.kind, Kind::Slide | Kind::SlideEnd) {
            Some(l / SLIDE_V)
        } else if b.kind == Kind::Graze {
            let Tag::Graze(gv) = b.tag else {
                unreachable!("a graze's speed")
            };
            Some(l / gv)
        } else if b.kind == Kind::Cross || a.kind == Kind::Cross {
            let slot = (b.kind == Kind::Cross && a.kind == Kind::SlotIn)
                || (a.kind == Kind::Cross && b.kind == Kind::SlotOut);
            Some(l / if slot { SLOTV } else { SLOW })
        } else if a.kind == Kind::Slow && b.kind == Kind::Slow {
            Some(l / (1.6 * SLOW))
        } else if b.kind == Kind::Wall {
            Some(l / WALL_IN)
        } else {
            free_len += l;
            None
        };
        if let Some(d) = dt {
            fixed_t += d;
        }
        segs.push((b, l, dt));
    }
    let dur = match speed {
        Some(s) => fixed_t + free_len / s,
        None => dur,
    };
    let left = dur - fixed_t;
    if left < 0.06 {
        return None;
    }
    let speed = free_len / left;
    if !(lo <= speed && speed <= 760.0) {
        return None;
    }
    let mut t = t0;
    let mut out = Vec::new();
    for (b, l, dt) in segs {
        t += dt.unwrap_or(l / speed);
        out.push((t, b));
    }
    Some((out, speed))
}

struct Applied {
    keys: Vec<Key>,
    cross: Vec<(f64, f64, &'static str)>,
    contact: Vec<(f64, f64)>,
    walls: Vec<WallHit>,
    squeeze: Vec<(f64, f64)>,
    graze: Option<GrazeRec>,
}

fn apply_move(timed: &[(f64, Way)], st: &mut State) -> Applied {
    let (mut keys, mut cross, mut contact, mut walls) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::<WallHit>::new());
    let mut squeeze: Vec<(f64, Option<f64>)> = Vec::new();
    let (mut g_first, mut g_last, mut t_in) = (None, 0.0, f64::NAN);
    for &(t, b) in timed {
        let v = if matches!(b.kind, Kind::Wall | Kind::Hold) {
            Some((0.0, 0.0))
        } else {
            None
        };
        keys.push(Key {
            t,
            x: b.x,
            y: b.y,
            v,
        });
        if matches!(b.kind, Kind::Cross | Kind::XFree) {
            let Tag::Portal(nm) = b.tag else {
                unreachable!("a crossing's portal")
            };
            cross.push((t, ramp_of(nm), nm));
        }
        if b.kind == Kind::SlotIn {
            squeeze.push((t, None));
        }
        if b.kind == Kind::SlotOut
            && let Some(s) = squeeze.last_mut()
        {
            s.1 = Some(t);
        }
        if b.kind == Kind::Wall {
            let Tag::Wall(nx, ny) = b.tag else {
                unreachable!("a wall's normal")
            };
            walls.push(WallHit {
                t,
                p: (b.x, b.y),
                n: (nx, ny),
                t_hold: f64::NAN,
                t_slide: f64::NAN,
            });
        }
        if b.kind == Kind::Hold {
            walls.last_mut().expect("a wall before its hold").t_hold = t;
        }
        if b.kind == Kind::SlideEnd {
            walls.last_mut().expect("a wall before its slide").t_slide = t;
        }
        if b.kind == Kind::ContactIn {
            t_in = t;
        }
        if b.kind == Kind::Graze {
            if g_first.is_none() {
                g_first = Some(t);
            }
            g_last = t;
        }
    }
    for w in &walls {
        contact.push((w.t - 0.05, w.t_slide));
    }
    let mut graze = None;
    if g_first.is_some() {
        let gm = st.grazes.last_mut().expect("the graze laid");
        gm.t0 = t_in;
        contact.push((t_in - 0.01, g_last + 0.02));
        graze = Some(gm.clone());
    }
    let squeeze = squeeze
        .into_iter()
        .filter_map(|(a, b)| b.map(|b| (a, b)))
        .collect();
    Applied {
        keys,
        cross,
        contact,
        walls,
        squeeze,
        graze,
    }
}

impl Plasma {
    /// Sample the head over [t0, t1]: never behind by the block, never in
    /// the plane inside a stroke, and inside the envelope.
    fn validate(&self, ch: &Chain, t0: f64, t1: f64) -> Option<&'static str> {
        let mut t = t0;
        while t <= t1 + 1e-9 {
            let (x, y) = ch.xy(t);
            let z = ch.z(t);
            if !(ENVELOPE.0 <= x && x <= ENVELOPE.2 && ENVELOPE.1 <= y && y <= ENVELOPE.3) {
                return Some("envelope");
            }
            if z < 3.0 && off_block(x, y) < HW + 4.5 {
                return Some("block");
            }
            if z.abs() < CONTACT && self.sdf_u(x, y) < 1.2 {
                return Some("ink");
            }
            if z.abs() < CONTACT && !self.in_hole(x, y) && self.sdf_u(x, y) < 1.2 {
                return Some("ink");
            }
            t += 0.005;
        }
        None
    }
}

/// The mix, steered by what the chain has so far: contact toward 28%,
/// time behind toward 45%, and at least three hole passes, a hit, a weave
/// and a turnaround by the end. Still a random pick.
fn director(ch: &Chain, t: f64, sign_now: i64) -> Vec<(&'static str, f64)> {
    let mut w: Vec<(&'static str, f64)> = MIX.to_vec();
    if t < 0.4 {
        return w;
    }
    let mul = |w: &mut Vec<(&'static str, f64)>, k: &str, f: f64| {
        let e = w.iter_mut().find(|e| e.0 == k).expect("a move");
        e.1 *= f;
    };
    let contact = py::sum(ch.contact.iter().map(|&(a, b)| b - a)).unwrap_or(0.0);
    let mut cr: Vec<f64> = ch.cross.iter().map(|c| c.0).collect();
    cr.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
    let (mut behind, mut s, mut last) = (0.0, ch.s0, 0.0);
    for c in cr {
        if s < 0 {
            behind += c - last;
        }
        s = -s;
        last = c;
    }
    if s < 0 {
        behind += t - last;
    }
    if contact < 0.27 * t - 0.15 {
        mul(&mut w, "graze", 2.5);
        mul(&mut w, "hit", 1.8);
    } else if contact > 0.31 * t + 0.15 {
        mul(&mut w, "graze", 0.25);
        mul(&mut w, "hit", 0.4);
    }
    let share = behind / t;
    let flip = (sign_now > 0 && share < 0.43) || (sign_now < 0 && share > 0.47);
    if flip {
        mul(&mut w, "drop", 2.5);
        mul(&mut w, "sweep", 1.4);
    } else {
        mul(&mut w, "drop", 0.4);
    }
    let count = |k: &str| ch.moves.iter().filter(|m| m.kind == k).count();
    let holes = ch
        .cross
        .iter()
        .filter(|c| hole(c.2).is_some() || c.2 == "u")
        .count();
    let late = t / LOOP;
    if holes < 3 {
        #[allow(clippy::cast_possible_wrap, reason = "under three")]
        let more = fl(3 - holes as i64);
        mul(&mut w, "drop", 1.0 + 2.0 * late * more);
    }
    for kind in ["hit", "weave", "turn"] {
        if count(kind) == 0 {
            mul(&mut w, kind, 1.0 + 4.0 * late);
        }
    }
    w
}

impl Plasma {
    #[allow(
        clippy::too_many_lines,
        reason = "the move loop, as the generator wrote it"
    )]
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "small counts"
    )]
    fn make_chain(&self, seed: u64) -> Result<Chain, Stop> {
        let skip = || Stop::Exit(format!("seed {seed}: no move fits"));
        let mut r = Rng::new(seed);
        let mut ch = Chain::default();
        let y0 = r.r(52.0, 82.0);
        let x0 = 24.0;
        ch.s0 = if r.u() < 0.5 { 1 } else { -1 };
        ch.keys.push(Key {
            t: 0.0,
            x: x0,
            y: y0,
            v: None,
        });
        let mut st = State {
            p: (x0, y0),
            p_np: false,
            last_y: [Some(y0), None],
            recent: Vec::new(),
            grazes: Vec::new(),
        };
        let mut t = 0.0;
        let mut k: u64 = 0;
        loop {
            let remaining = LOOP - t;
            let closing = remaining < 2.0;
            let mut found = None;
            for attempt in 0..60u64 {
                let mut rp = Rng::new(seed * 1009 + k * 97 + attempt);
                let sign_now = if ch.cross.len() % 2 == 0 {
                    ch.s0
                } else {
                    -ch.s0
                };
                let (kind, dur, w, side) = if closing {
                    let (x, y) = st.p;
                    let mut w = Vec::new();
                    if sign_now != ch.s0 {
                        let mut cand: Vec<(&'static str, f64)> = PORTAL_W
                            .iter()
                            .copied()
                            .filter(|&(a, _)| {
                                !st.recent.contains(&a) && a != "endR" && portal_x(a) < x - 4.0
                            })
                            .collect();
                        if cand.is_empty() {
                            cand = vec![("endL", 1.0)];
                        }
                        let nm = rp.pick(&cand);
                        w.extend(portal_way(nm, &mut rp, -1.0, y));
                    }
                    let nd = int(rp.u() * 3.0);
                    for _ in 0..nd {
                        let a = rp.r(30.0, 250.0);
                        w.push(free(a, rp.r(28.0, 106.0)));
                    }
                    w.push(free(x0, y0));
                    ("close", remaining, w, Some(0))
                } else {
                    let kind = rp.pick(&director(&ch, t, sign_now));
                    let ng = st.grazes.len();
                    let got = match kind {
                        "sweep" => Self::mv_sweep(&mut rp, &st),
                        "turn" => Self::mv_turn(&mut rp, &st),
                        "drop" => Self::mv_drop(&mut rp, &st),
                        "weave" => Self::mv_weave(&mut rp, &st),
                        "graze" => self.mv_graze(&mut rp, &mut st),
                        _ => self.mv_hit(&mut rp, &st),
                    };
                    let Some((w, side)) = got else {
                        st.grazes.truncate(ng);
                        continue;
                    };
                    let u = rp.u();
                    let dur = if kind != "graze" && kind != "hit" {
                        0.3 + 1.5 * u * u
                    } else {
                        rp.r(0.5, 1.6)
                    };
                    if t + dur > LOOP - 0.35 {
                        st.grazes.truncate(ng);
                        continue;
                    }
                    (kind, dur, w, side)
                };
                let contactful = kind == "graze" || kind == "hit";
                let lo = if kind == "close" { 60.0 } else { 90.0 };
                let speed = if contactful {
                    Some(rp.r(220.0, 520.0))
                } else {
                    None
                };
                let mut got = time_way(&w, st.p, t, dur, lo, speed);
                if let Some((timed, _)) = &got
                    && contactful
                    && timed[timed.len() - 1].0 > LOOP - 0.35
                {
                    got = None;
                }
                let Some((timed, speed)) = got else {
                    if kind == "graze" {
                        st.grazes.pop();
                    }
                    continue;
                };
                let ap = apply_move(&timed, &mut st);
                // the closing move lands on the start key: drop it, the chain is cyclic
                let mut trial = Chain {
                    s0: ch.s0,
                    ..Chain::default()
                };
                trial.keys = ch.keys.iter().chain(&ap.keys).copied().collect();
                trial.cross = ch.cross.iter().chain(&ap.cross).copied().collect();
                trial.contact = ch.contact.iter().chain(&ap.contact).copied().collect();
                trial.prepare();
                if self
                    .validate(&trial, py::max(0.0, t - 0.1), timed[timed.len() - 1].0)
                    .is_some()
                {
                    if ap.graze.is_some() {
                        st.grazes.pop();
                    }
                    continue;
                }
                let _ = speed;
                found = Some((kind, side, timed, ap));
                break;
            }
            let Some((kind, side, timed, ap)) = found else {
                return Err(skip());
            };
            ch.keys.extend(&ap.keys);
            ch.cross.extend(&ap.cross);
            ch.contact.extend(&ap.contact);
            ch.walls.extend(ap.walls.iter().cloned());
            ch.squeeze.extend(&ap.squeeze);
            if let Some(g) = &ap.graze {
                ch.grazes.push(g.clone());
            }
            let x_start = st.p.0;
            let t_end = timed[timed.len() - 1].0;
            let last = ap.keys[ap.keys.len() - 1];
            // A waypoint off a contour walk is NumPy's float64, whose
            // `round` is NumPy's.
            let end_np = kind == "graze" || kind == "hit";
            let round1 = |v: f64, np: bool| {
                if np {
                    numpy::round_to(v, 1)
                } else {
                    py::round_to(v, 1)
                }
            };
            ch.moves.push(Move {
                kind,
                dur: py::round_to(t_end - t, 3),
                x0: round1(x_start, st.p_np),
                x1: round1(last.x, end_np),
            });
            st.p = (last.x, last.y);
            st.p_np = end_np;
            if let Some(s) = side {
                st.last_y[s] = Some(last.y);
            }
            st.recent = ap.cross.iter().map(|c| c.2).collect();
            t = t_end;
            k += 1;
            if kind == "close" {
                break;
            }
        }
        let last = ch.keys.pop().expect("the closing key");
        if !((last.t - LOOP).abs() < 1e-6
            && (last.x - x0).abs() < 1e-9
            && (last.y - y0).abs() < 1e-9)
        {
            return Err(Stop::Assert(format!(
                "({}, {}, {})",
                last.t, last.x, last.y
            )));
        }
        if ch.cross.len() % 2 != 0 {
            return Err(Stop::Assert("depth parity round the loop".into()));
        }
        ch.cyclic = true;
        ch.prepare();
        if let Some(why) = self.validate(&ch, 0.0, LOOP) {
            return Err(Stop::Exit(format!(
                "seed {seed}: the closed loop fails: {why}"
            )));
        }
        Ok(ch)
    }
}

/// The chain's stats, and whether they meet the targets.
#[allow(clippy::cast_precision_loss, reason = "small counts")]
fn stats(ch: &Chain) -> (String, bool) {
    let ts = numpy::arange(0.0, LOOP, 0.01);
    let zs: Vec<f64> = ts
        .iter()
        .map(|&t| if ch.z(t) < 0.0 { 1.0 } else { 0.0 })
        .collect();
    let behind = numpy::mean(&zs);
    let cs: Vec<f64> = ts
        .iter()
        .map(|&t| if ch.in_contact(t, 0.0) { 1.0 } else { 0.0 })
        .collect();
    let contact = numpy::mean(&cs);
    let durs: Vec<f64> = ch.moves.iter().map(|m| m.dur).collect();
    let mean = det::fsum_mean(&durs);
    let sq: Vec<f64> = durs.iter().map(|&d| (d - mean) * (d - mean)).collect();
    let cv = det::fsum_mean(&sq).sqrt() / mean;
    let same = ch
        .moves
        .iter()
        .filter(|m| (m.x0 < 147.0) == (m.x1 < 147.0))
        .count();
    let holes = ch
        .cross
        .iter()
        .filter(|c| hole(c.2).is_some() || c.2 == "u")
        .count();
    let slots = ch.cross.iter().filter(|c| slot(c.2).is_some()).count();
    let mut kinds: Vec<(&str, usize)> = Vec::new();
    for m in &ch.moves {
        match kinds.iter_mut().find(|e| e.0 == m.kind) {
            Some(e) => e.1 += 1,
            None => kinds.push((m.kind, 1)),
        }
    }
    let dmin = durs.iter().copied().fold(f64::INFINITY, f64::min);
    let dmax = durs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let r3 = |v: f64| py::round_to(v, 3);
    let (behind, contact, cv) = (r3(behind), r3(contact), r3(cv));
    let same_side = r3(same as f64 / ch.moves.len() as f64);
    let kind_n = |k: &str| kinds.iter().find(|e| e.0 == k).map_or(0, |e| e.1);
    let fits = (0.40..=0.50).contains(&behind)
        && (0.25..=0.32).contains(&contact)
        && (0.30..=0.60).contains(&same_side)
        && cv >= 0.30
        && holes >= 3
        && kind_n("weave") >= 1
        && !ch.walls.is_empty()
        && kind_n("turn") >= 1;
    let kinds = kinds
        .iter()
        .map(|(k, n)| format!("\"{k}\": {n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let r = py::repr_float;
    let json = format!(
        "{{\"behind\": {}, \"contact\": {}, \"moves\": {}, \"dur_cv\": {}, \"dur_min\": {}, \"dur_max\": {}, \
         \"same_side\": {}, \"holes\": {holes}, \"slots\": {slots}, \"crossings\": {}, \"grazes\": {}, \
         \"hits\": {}, \"kinds\": {{{kinds}}}}}",
        r(behind),
        r(contact),
        ch.moves.len(),
        r(cv),
        r(r3(dmin)),
        r(r3(dmax)),
        r(same_side),
        ch.cross.len(),
        ch.grazes.len(),
        ch.walls.len(),
    );
    (json, fits)
}

fn surge_shape(dt: f64) -> f64 {
    if dt < -1.0 {
        return 0.0;
    }
    if dt < 0.0 {
        let x = dt + 1.0;
        return 0.25 * x * x;
    }
    if dt < 0.12 {
        let x = dt / 0.12;
        return 0.25 + 0.4 * x * x * (3.0 - 2.0 * x);
    }
    0.65 * det::exp(-(dt - 0.12) / 0.35)
}

fn ramp(r: f64) -> f64 {
    let r = numpy::clip(r, 0.0, 1.0);
    let r3 = r * r * r;
    15.0 / 8.0 * r - 10.0 / 8.0 * r3 + 3.0 / 8.0 * (r3 * r * r)
}

impl Built {
    fn surge(&self, t: f64) -> f64 {
        let mut s = 0.0;
        for &r0 in &self.surges {
            let mut dt = t - r0;
            dt -= LOOP * (dt / LOOP + 0.5).floor();
            s += surge_shape(dt);
        }
        s
    }

    fn in_surge(&self, t: f64) -> bool {
        self.surges.iter().any(|&r0| {
            let mut dt = t - r0;
            dt -= LOOP * (dt / LOOP + 0.5).floor();
            (0.0..0.4).contains(&dt)
        })
    }

    /// The stream function, exactly periodic in 10 s.
    fn psi(&self, x: f64, y: f64, t: f64) -> f64 {
        let mut s = 0.0;
        for &(a, b, j, ph, amp) in &self.waves {
            s += amp * det::sin(2.0 * std::f64::consts::PI * (a * x + b * y + j * t / LOOP + ph));
        }
        s
    }

    /// Bridson's curl noise, the potential ramped to zero at the slabs' 3D
    /// distance so the flow runs along them, never into them.
    fn flow(&self, pl: &Plasma, x: f64, y: f64, z: f64, t: f64, flow_k: f64) -> (f64, f64) {
        let rt = 1.5;
        let pp = |xx: f64, yy: f64| {
            let a = numpy::maximum(pl.sdf_field(xx, yy) / 2.0 - rt, 0.0);
            let b = numpy::maximum(z.abs() - TAU / 2.0, 0.0);
            let dd = (a * a + b * b).sqrt();
            ramp(dd / 10.0) * self.psi(xx, yy, t)
        };
        let h = 0.5;
        let dpdy = (pp(x, y + h) - pp(x, y - h)) / (2.0 * h);
        let dpdx = (pp(x + h, y) - pp(x - h, y)) / (2.0 * h);
        (flow_k * dpdy, -flow_k * dpdx)
    }

    fn head_z(&self, t: f64) -> f64 {
        self.ch.z(t)
    }

    fn head_v(&self, t: f64) -> (f64, f64) {
        let (a, b) = (self.ch.xy(t - 0.005), self.ch.xy(t + 0.005));
        ((b.0 - a.0) / 0.01, (b.1 - a.1) / 0.01)
    }

    /// Where the head is drawn: the chain, carried by the current.
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "2000 samples"
    )]
    fn head_draw(&self, t: f64) -> (f64, f64) {
        let (x, y) = self.ch.xy(t);
        let i = py::rem(t, LOOP) / 0.005;
        let n = self.disp.len() as i64;
        let i0 = int(i.floor()).rem_euclid(n) as usize;
        let i1 = (i0 + 1) % self.disp.len();
        let f = i - i.floor();
        let d = (
            self.disp[i0].0 * (1.0 - f) + self.disp[i1].0 * f,
            self.disp[i0].1 * (1.0 - f) + self.disp[i1].1 * f,
        );
        (x + d.0, y + d.1)
    }
}

impl Plasma {
    #[allow(
        clippy::too_many_lines,
        reason = "the module's top level, in its order"
    )]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap,
        reason = "small counts"
    )]
    fn build(&self) -> Result<Built, Stop> {
        let mut found = None;
        for sd in SEED..SEED + 400 {
            match self.make_chain(sd) {
                Ok(c) => {
                    let (s, fits) = stats(&c);
                    if fits {
                        found = Some((c, s, sd));
                        break;
                    }
                }
                Err(Stop::Exit(_)) => {}
                Err(e) => return Err(e),
            }
        }
        let Some((ch, stats, seed)) = found else {
            return Err(Stop::Exit(
                "plasma: no chain from its seed meets the targets".into(),
            ));
        };
        // surges: three a loop, at seeded times, 2 s apart at least
        let mut rs = Rng::new(9001 + seed);
        let mut surges: Vec<f64> = Vec::new();
        while surges.len() < 3 {
            let x = rs.r(0.3, 9.7);
            if surges
                .iter()
                .all(|&y| py::min((x - y).abs(), LOOP - (x - y).abs()) >= 2.0)
            {
                surges.push(x);
            }
        }
        surges.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
        // the current
        let mut rw = Rng::new(424_242 + seed);
        let mut waves = Vec::new();
        for (lam, amp) in [(30.0, 1.0), (15.0, 0.55), (8.0, 0.3)] {
            for _ in 0..4 {
                let th = 2.0 * std::f64::consts::PI * rw.u();
                let j = int(4.0 + rw.u() * 14.0);
                let j = j * if rw.u() < 0.5 { 1 } else { -1 };
                let ph = rw.u();
                waves.push((det::cos(th) / lam, det::sin(th) / lam, fl(j), ph, amp * lam));
            }
        }
        let mut b = Built {
            ch,
            seed,
            stats,
            surges,
            waves,
            flow_k: 1.0,
            disp: Vec::new(),
            tongues: Vec::new(),
            res: Vec::new(),
            parcels: Parcels {
                x: Vec::new(),
                y: Vec::new(),
                z: Vec::new(),
                hug: Vec::new(),
            },
        };
        // scale the flow to an rms speed of 170 u/s over the logo
        let gxs = numpy::arange(30.0, 270.0, 4.0);
        let gys = numpy::arange(30.0, 104.0, 4.0);
        let mut sq = Vec::new();
        for &gy in &gys {
            for &gx in &gxs {
                let (fx, fy) = b.flow(self, gx, gy, 20.0, 0.0, 1.0);
                sq.push(fx * fx + fy * fy);
            }
        }
        b.flow_k = 170.0 / det::fsum_mean(&sq).sqrt();
        b.disp = self.displacement(&b);
        b.tongues = self.tongue_paths(&b.ch);
        b.res = self.residual_segments(seed);
        b.parcels = self.simulate_parcels(&b);
        Ok(b)
    }

    /// The head carried by the flow: D' = flow(H + D) - 2.5 D on a 5 ms grid
    /// over one lap from D = 0; the flow is not contracting, so the lap's
    /// end misses its start, and the miss is taken off linearly, D less
    /// t / 10 s of D(10 s), so the loop closes. Held to zero round each crossing and
    /// contact.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap,
        reason = "2000 samples"
    )]
    fn displacement(&self, b: &Built) -> Vec<(f64, f64)> {
        let dt = 0.005;
        let n = usize::try_from(py::round(LOOP / dt)).expect("2000");
        let mut d = (0.0, 0.0);
        let mut out = vec![(0.0, 0.0); n + 1];
        for i in 0..n {
            let t = i as f64 * dt;
            let (hx, hy) = b.ch.xy(t);
            let z = b.ch.z(t);
            let (fx, fy) = b.flow(self, hx + d.0, hy + d.1, z, t, b.flow_k);
            d = (d.0 + dt * (fx - 2.5 * d.0), d.1 + dt * (fy - 2.5 * d.1));
            out[i + 1] = d;
        }
        let miss = out[n];
        let out: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let k = i as f64 / n as f64;
                (out[i].0 - k * miss.0, out[i].1 - k * miss.1)
            })
            .collect();
        let mut g = vec![1.0; n];
        for (i, gi) in g.iter_mut().enumerate() {
            let t = i as f64 * dt;
            let mut near = 9.0;
            let mut first = true;
            for c in
                b.ch.cross
                    .iter()
                    .map(|c| (py::rem((t - c.0) + LOOP / 2.0, LOOP) - LOOP / 2.0).abs())
            {
                near = if first { c } else { py::min(near, c) };
                first = false;
            }
            near = py::min(near, 9.0);
            let mut w = smooth(0.06, 0.22, near);
            if b.ch.in_contact(t, 0.08) || b.ch.in_squeeze(t) {
                w = 0.0;
            }
            *gi = w;
        }
        // a light low-pass on the gate so it never steps
        let w21 = 1.0 / 21.0;
        let ni = n as i64;
        #[allow(clippy::cast_sign_loss, reason = "rem_euclid is never negative")]
        let gs: Vec<f64> = (0..ni)
            .map(|i| py::fsum((-10..=10).map(|d| g[(i + d).rem_euclid(ni) as usize] * w21)))
            .collect();
        out.iter()
            .zip(&gs)
            .map(|(o, &s)| (o.0 * s, o.1 * s))
            .collect()
    }

    /// Each contact's two tongues, along the stroke both ways, at a close iso.
    #[allow(clippy::cast_possible_wrap, reason = "a few contacts")]
    fn tongue_paths(&self, ch: &Chain) -> Vec<Tongue> {
        let mut out = Vec::new();
        let iso = GRAZE_C * 2.0 - 7.0;
        for (i, w) in ch.walls.iter().enumerate() {
            let pa = self.contour_walk(w.p, 1.0, 80.0, iso, 1.0);
            let pb = self.contour_walk(w.p, -1.0, 80.0, iso, 1.0);
            out.push(Tongue {
                t: w.t,
                life: 0.36,
                speed: 320.0,
                paths: [pa, pb],
                gain: 1.3,
                id: i as i64,
            });
        }
        for (i, g) in ch.grazes.iter().enumerate() {
            // ahead of the graze, the light runs on along the stroke; behind it, a shorter one
            let p = g.walk[0];
            let pa = self.contour_walk(p, g.sg, 90.0, iso, 1.0);
            let pb = self.contour_walk(p, -g.sg, 45.0, iso, 1.0);
            out.push(Tongue {
                t: g.t0 + 0.02,
                life: 0.34,
                speed: 300.0,
                paths: [pa, pb],
                gain: 1.0,
                id: 100 + i as i64,
            });
        }
        for c in &mut out {
            for pts in &mut c.paths {
                if let Some(cut) = pts
                    .iter()
                    .position(|p| off_block(p.0, p.1) < 10.0 || self.in_hole(p.0, p.1))
                {
                    pts.truncate(cut);
                }
            }
        }
        out
    }

    /// Points on two iso-lines (3.5 u and 6.5 u off the strokes, and inside
    /// the counters), 5 u apart; each its own seeded flickers.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap,
        reason = "canvas indices"
    )]
    fn residual_segments(&self, seed: u64) -> Vec<Res> {
        let mut segs = Vec::new();
        for (iso_u, spacing) in [(3.5, 5.5), (6.5, 9.0)] {
            let iso = iso_u * 2.0;
            let (mut us, mut vs) = (Vec::new(), Vec::new());
            for y in 0..H_ {
                for x in 0..W_ {
                    if (self.sdf[y * W_ + x] - iso).abs() < 0.5 {
                        let (u, v) = (x as f64 / 2.0 - 60.0, y as f64 / 2.0 - 75.0);
                        if u > 40.0 && u < 236.0 && v > 30.0 && v < 104.0 {
                            us.push(u);
                            vs.push(v);
                        }
                    }
                }
            }
            let mut grid: std::collections::HashMap<(i64, i64), Vec<(f64, f64)>> =
                std::collections::HashMap::new();
            for i in numpy::lexsort(&us, &vs) {
                let (u, v) = (us[i], vs[i]);
                if off_block(u, v) < 9.0 {
                    continue;
                }
                let gk = (int(py::floordiv(u, spacing)), int(py::floordiv(v, spacing)));
                let mut close = false;
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        if let Some(cell) = grid.get(&(gk.0 + dx, gk.1 + dy)) {
                            for &(a, b) in cell {
                                if (a - u) * (a - u) + (b - v) * (b - v) < spacing * spacing {
                                    close = true;
                                }
                            }
                        }
                    }
                }
                if close {
                    continue;
                }
                grid.entry(gk).or_default().push((u, v));
                segs.push((u, v));
            }
        }
        let mut out = Vec::new();
        for (j, &(u, v)) in segs.iter().enumerate() {
            let mut r = Rng::new(0x5E_0000 + j as u64 + 7919 * seed);
            let (nx, ny) = self.normal(u, v);
            let n = 6 + int(r.u() * 10.0);
            let mut ev = Vec::new();
            for _ in 0..n {
                let a = r.r(0.0, LOOP);
                let b = r.r(0.16, 0.6);
                let c = r.r(0.5, 1.0);
                let d = r.r(-18.0, 18.0);
                ev.push((a, b, c, d, r.r(5.0, 9.0)));
            }
            out.push(Res {
                p: (u, v),
                t: (-ny, nx),
                ev,
                id: j as i64,
            });
        }
        out
    }

    /// Three strands of parcels, one every 12.5 ms each, off the head and
    /// 7 u either side of it; each advected by the bounded flow for its
    /// life, settling onto the slabs' face and sliding along a stroke where
    /// it meets one there. Positions every 5 ms; column j = strand * NCH + k.
    #[allow(clippy::cast_precision_loss, reason = "small counts")]
    fn simulate_parcels(&self, b: &Built) -> Parcels {
        const OFF: [f64; 3] = [0.0, -7.0, 7.0];
        let nch = nch();
        let births: Vec<f64> = (0..3 * nch).map(|j| (j % nch) as f64 * STEP).collect();
        let steps = idx(0.56 / 0.005) + 1;
        let n = births.len();
        let hp: Vec<(f64, f64)> = births[..nch].iter().map(|&t| b.head_draw(t)).collect();
        let v0: Vec<(f64, f64)> = births[..nch].iter().map(|&t| b.head_v(t)).collect();
        let sp: Vec<f64> = v0
            .iter()
            .map(|v| numpy::maximum(det::hypot(v.0, v.1), 1e-6))
            .collect();
        let nxh: Vec<f64> = (0..nch).map(|k| -v0[k].1 / sp[k]).collect();
        let nyh: Vec<f64> = (0..nch).map(|k| v0[k].0 / sp[k]).collect();
        let off = |j: usize| OFF[j / nch];
        let mut x: Vec<f64> = (0..n)
            .map(|j| hp[j % nch].0 + off(j) * nxh[j % nch])
            .collect();
        let mut y: Vec<f64> = (0..n)
            .map(|j| hp[j % nch].1 + off(j) * nyh[j % nch])
            .collect();
        let zk: Vec<f64> = births[..nch].iter().map(|&t| b.head_z(t)).collect();
        let mut z: Vec<f64> = (0..n).map(|j| zk[j % nch]).collect();
        let vx: Vec<f64> = (0..n).map(|j| 0.3 * v0[j % nch].0).collect();
        let vy: Vec<f64> = (0..n).map(|j| 0.3 * v0[j % nch].1).collect();
        let mut px = vec![vec![0.0; n]; steps];
        let mut py_ = vec![vec![0.0; n]; steps];
        let mut pz = vec![vec![0.0; n]; steps];
        let mut hug = vec![vec![0.0; n]; steps];
        px[0].clone_from(&x);
        py_[0].clone_from(&y);
        pz[0].clone_from(&z);
        let settle = det::exp(-0.005 / 0.08);
        for s in 1..steps {
            let age = (s - 1) as f64 * 0.005;
            let dec = det::exp(-age / 0.06);
            for j in 0..n {
                let tt = py::rem(births[j] + age, LOOP);
                let (fx, fy) = b.flow(self, x[j], y[j], z[j], tt, b.flow_k);
                let mut ux = fx + vx[j] * dec;
                let mut uy = fy + vy[j] * dec;
                let sd = self.sdf_field(x[j], y[j]) / 2.0;
                let inplane = z[j].abs() < CONTACT;
                let (nx_, ny_) = self.nrm_field(x[j], y[j]);
                let into = ux * nx_ + uy * ny_;
                // the wall: the normal part taken off, the rest kept, so it slides along
                if inplane && sd < 6.0 && into < 0.0 {
                    ux -= into * nx_;
                    uy -= into * ny_;
                }
                x[j] += 0.005 * ux;
                y[j] += 0.005 * uy;
                let sd = self.sdf_field(x[j], y[j]) / 2.0;
                let (nx_, ny_) = self.nrm_field(x[j], y[j]);
                if inplane && sd < 2.5 {
                    x[j] += nx_ * (2.5 - sd);
                    y[j] += ny_ * (2.5 - sd);
                }
                // the depth settles onto the slabs' face, never through it
                if z[j].abs() > 3.0 {
                    let sign = if z[j] > 0.0 {
                        1.0
                    } else if z[j] < 0.0 {
                        -1.0
                    } else {
                        0.0
                    };
                    z[j] = sign * numpy::maximum(z[j].abs() * settle, 3.0);
                }
                px[s][j] = x[j];
                py_[s][j] = y[j];
                pz[s][j] = z[j];
                hug[s][j] = if z[j].abs() < CONTACT && self.sdf_field(x[j], y[j]) / 2.0 < 7.0 {
                    1.0
                } else {
                    0.0
                };
            }
        }
        Parcels {
            x: px,
            y: py_,
            z: pz,
            hug,
        }
    }
}

// ---- the frames ---------------------------------------------------------

const NSTRAND: usize = 3;
const PLIFE: f64 = 0.56;
const FADE_IN: f64 = 0.3;
const FADE_OUT: f64 = 0.4;
const FILM_CS: i64 = 450;

impl Built {
    /// A chord's front weight off its frozen z; in the plane against a
    /// stroke it is drawn behind (the keyline parts it from the stroke),
    /// except by the block, where nothing is ever behind.
    fn front_of(&self, z: f64, x: f64, y: f64, t: f64) -> f64 {
        let mut f = smooth(-3.0, 3.0, z);
        if self.ch.in_contact(py::rem(t, LOOP), 0.04) && z.abs() < CONTACT {
            f = if off_block(x, y) > HW + 5.0 { 0.0 } else { 1.0 };
        }
        if f < 1.0 && off_block(x, y) < HW + 4.5 {
            f = 1.0;
        }
        f
    }

    /// In the plane, or threading a channel, the chord fits the opening:
    /// its width min(w, sdf - keyline) and its light sqrt(w / w0).
    fn squeezed(&self, pl: &Plasma, x: f64, y: f64, z: f64, t: f64, width: f64) -> (f64, f64) {
        let tt = py::rem(t, LOOP);
        if z.abs() < CONTACT || self.ch.in_squeeze(tt) {
            let s_px = pl.sdf(x, y);
            let k = py::min(2.0, 0.3 * py::max(s_px, 0.0));
            let w = py::max(2.5, py::min(width, s_px - k));
            return (w, (w / width).sqrt());
        }
        (width, 1.0)
    }

    /// Out of the ink, against a stroke only.
    fn push(&self, pl: &Plasma, x: f64, y: f64, z: f64, t: f64, r_u: f64) -> (f64, f64) {
        if z.abs() >= CONTACT || !self.ch.in_contact(py::rem(t, LOOP), 0.04) {
            return (x, y);
        }
        let s = pl.sdf(x, y);
        let need = r_u * 0.6 * 2.0 + 1.5;
        if s < need {
            let (nx, ny) = pl.normal(x, y);
            let d = (need - s) / 2.0;
            return (x + nx * d, y + ny * d);
        }
        (x, y)
    }

    /// In front over the block, the plasma's body would take its amber
    /// away: a chord whose body reaches the block is dimmed toward 0.12 of
    /// itself, so the block stays amber when shown.
    fn block_dim(x: f64, y: f64, front: f64) -> f64 {
        if front <= 0.5 {
            return 1.0;
        }
        0.12 + 0.88 * smooth(1.0, HW + 6.0, off_block(x, y))
    }

    /// The head and its tail: chord k laid at k * 12.5 ms, its frozen
    /// depth, oldest first. `t_ms2`: the frame's time in half-milliseconds.
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "800 chords"
    )]
    fn chain_chords(&self, pl: &Plasma, t_ms2: i64, out: &mut Vec<String>, tail: f64) {
        let t = fl(t_ms2) / 2000.0;
        let klast = t_ms2.div_euclid(25);
        let kfirst = klast - int(tail / STEP);
        let nch = nch() as i64;
        for k in kfirst..=klast {
            let age2 = t_ms2 - 25 * k;
            let age = fl(age2) / 2000.0;
            if age < 0.0 || age >= tail {
                continue;
            }
            let kk = k.rem_euclid(nch);
            let s = fl(kk) * STEP;
            let newest = age < STEP;
            let sa = s - 0.015;
            let sb = if newest { s + age } else { s + 0.015 };
            let mut b = self.head_draw(sa);
            let mut f = self.head_draw(sb);
            let z = self.head_z(s);
            let mut wall = None;
            for w in &self.ch.walls {
                for off in [0.0, -LOOP, LOOP] {
                    if w.t - 0.005 <= s + off && s + off <= w.t_hold + 0.01 {
                        wall = Some((w, s + off - w.t));
                    }
                }
            }
            let (width, light_k);
            if let Some((w, tau)) = wall {
                let (nx, ny) = w.n;
                let (tx, ty) = (-ny, nx);
                let e = ease(tau / 0.10);
                let st = 1.0 + 1.6 * e;
                let l = 0.5 * P_WIDTH * st;
                let (px_, py_) = (w.p.0 - nx * 3.0 * e, w.p.1 - ny * 3.0 * e);
                b = (px_ - tx * l, py_ - ty * l);
                f = (px_ + tx * l, py_ + ty * l);
                width = P_WIDTH * (1.0 - 0.5 * e);
                light_k = 1.35;
            } else {
                b = self.push(pl, b.0, b.1, z, s, HW);
                f = self.push(pl, f.0, f.1, z, s, HW);
                let (mx, my) = (0.5 * (b.0 + f.0), 0.5 * (b.1 + f.1));
                (width, light_k) = self.squeezed(pl, mx, my, z, s, P_WIDTH);
            }
            let (mx, my) = (0.5 * (b.0 + f.0), 0.5 * (b.1 + f.1));
            let front = self.front_of(z, mx, my, s);
            let mut weight = det::exp(-age / 0.07) + 0.6 * det::exp(-age / 0.04);
            weight *= Self::block_dim(mx, my, front);
            let p_age = 0.06 + 0.045 * age;
            let e = 1.0 - p_age / 0.55;
            let env = e * e;
            let light = pl.gain * weight / env
                * light_k
                * (1.0 + self.surge(if newest { py::rem(t, LOOP) } else { s }));
            let young = py::max(0.0, 1.0 - age / 0.35);
            let ions = 0.6 * if self.in_surge(s) { 2.0 } else { 1.0 };
            out.push(fmt(
                &[
                    front,
                    light,
                    b.0,
                    b.1,
                    f.0,
                    f.1,
                    width,
                    P_UNIT,
                    p_age,
                    age,
                    0.1 * young * young,
                    1.4,
                    5.0,
                    ions,
                ],
                0xE7_0000 + kk,
            ));
            if let Some((w, tau)) = wall
                && tau > 0.02
            {
                // the bulge: the squashed mass lips up and away at both ends
                let (nx, ny) = w.n;
                let (tx, ty) = (-ny, nx);
                let e = ease(tau / 0.10);
                for sgn in [-1.0, 1.0] {
                    let a0 = if sgn < 0.0 { b } else { f };
                    let a1 = (
                        a0.0 + sgn * tx * 5.0 * e + nx * 9.0 * e,
                        a0.1 + sgn * ty * 5.0 * e + ny * 9.0 * e,
                    );
                    out.push(fmt(
                        &[
                            front,
                            light * 0.7,
                            a0.0,
                            a0.1,
                            a1.0,
                            a1.1,
                            0.45 * P_WIDTH,
                            0.6 * P_UNIT,
                            p_age,
                            age,
                            0.1,
                            1.4,
                            5.0,
                            ions,
                        ],
                        0xE8_0000 + kk * 2 + i64::from(sgn > 0.0),
                    ));
                }
            }
        }
    }

    #[allow(clippy::cast_possible_wrap, reason = "a few hundred points")]
    fn tongues(&self, pl: &Plasma, t: f64, out: &mut Vec<String>) {
        for c in &self.tongues {
            for off in [0.0, LOOP, -LOOP] {
                let age = t - (c.t + off);
                if !(0.0 <= age && age < c.life) {
                    continue;
                }
                for (side, pts) in c.paths.iter().enumerate() {
                    let reach = c.speed * age * (1.0 - 0.5 * age / c.life);
                    let tip = int(reach).min(pts.len() as i64 - 1);
                    let fade = (1.0 - smooth(0.55 * c.life, c.life, age)) * smooth(0.0, 0.03, age);
                    if tip < 2 {
                        continue;
                    }
                    for m in 0..7i64 {
                        let a_i = 0.max(tip - (m + 1) * 6);
                        let b_i = 0.max(tip - m * 6);
                        if b_i <= a_i {
                            break;
                        }
                        #[allow(clippy::cast_sign_loss, reason = "never negative")]
                        let (pa, pb) = (pts[a_i as usize], pts[b_i as usize]);
                        let k = 1.0 - 0.6 * fl(m) / 7.0;
                        let light = pl.gain * c.gain * fade * k * if side == 1 { 0.6 } else { 1.0 };
                        let width = 7.0;
                        out.push(fmt(
                            &[
                                0.0,
                                light,
                                pa.0,
                                pa.1,
                                pb.0,
                                pb.1,
                                width,
                                12.0,
                                0.06 + 0.03 * age,
                                age,
                                0.05,
                                1.4,
                                5.0,
                                0.6,
                            ],
                            0xEA_0000 + (c.id * 16 + side as i64 * 8 + m) % 65536,
                        ));
                    }
                }
            }
        }
    }

    #[allow(clippy::cast_possible_wrap, reason = "a few events")]
    fn residual(&self, pl: &Plasma, t: f64, out: &mut Vec<String>) {
        let (hx, hy) = self.head_draw(t);
        for sgm in &self.res {
            let (u, v) = sgm.p;
            let (tx, ty) = sgm.t;
            let near = 1.0 + 1.6 * smooth(34.0, 10.0, det::hypot(u - hx, v - hy));
            for (m, &(t0, life, amp, crawl, l)) in sgm.ev.iter().enumerate() {
                for off in [0.0, LOOP, -LOOP] {
                    let age = t - (t0 + off);
                    if !(0.0 <= age && age < life) {
                        continue;
                    }
                    let w = det::sin(std::f64::consts::PI * age / life);
                    let w = w * w;
                    let light = pl.res_gain * amp * w * near;
                    if light < 2e-3 {
                        continue;
                    }
                    let c = (u + tx * crawl * age, v + ty * crawl * age);
                    let b = (c.0 - tx * l * 0.5, c.1 - ty * l * 0.5);
                    let f = (c.0 + tx * l * 0.5, c.1 + ty * l * 0.5);
                    out.push(fmt(
                        &[
                            0.0,
                            light,
                            b.0,
                            b.1,
                            f.0,
                            f.1,
                            3.0,
                            6.0,
                            0.06 + 0.04 * age,
                            age,
                            0.03,
                            1.4,
                            5.0,
                            0.3,
                        ],
                        0xE5_0000 + (sgm.id * 16 + m as i64) % 65536,
                    ));
                }
            }
        }
    }

    /// The tail as three ropes through the parcels, newest first, each link
    /// a chord tapering with age; a rope breaks where the flow has torn two
    /// parcels apart. A link hugging a stroke lives longer and brighter.
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "800 chords"
    )]
    fn parcel_rope(&self, pl: &Plasma, t_ms2: i64, out: &mut Vec<String>) {
        let t = fl(t_ms2) / 2000.0;
        let klast = t_ms2.div_euclid(25);
        let nch = nch() as i64;
        let pr = &self.parcels;
        let rows = pr.x.len();
        for strand in 0..NSTRAND as i64 {
            let mut prev: Option<((f64, f64), f64)> = None;
            let base_w = if strand == 0 { 12.0 } else { 8.0 };
            let gain = if strand == 0 { 1.0 } else { 0.7 };
            for k in (klast - int(PLIFE / STEP)..=klast).rev() {
                let age = fl(t_ms2 - 25 * k) / 2000.0;
                if !(0.0..PLIFE).contains(&age) {
                    prev = None;
                    continue;
                }
                let kk = k.rem_euclid(nch);
                let j = (strand * nch + kk) as usize;
                let si = (py::round(age / 0.005) as usize).min(rows - 1);
                let p = (pr.x[si][j], pr.y[si][j]);
                let zz = pr.z[si][j];
                let hug = if si > 0 {
                    let col: Vec<f64> = (si.saturating_sub(8)..=si).map(|r| pr.hug[r][j]).collect();
                    numpy::mean(&col)
                } else {
                    0.0
                };
                if let Some((q, qz)) = prev {
                    let d = det::hypot(p.0 - q.0, p.1 - q.1);
                    let life = 0.30 + 0.25 * hug;
                    let fade = py::max(0.0, 1.0 - age / life);
                    if 0.2 < d && d < 14.0 && fade > 0.01 {
                        let mut weight = (det::exp(-age / 0.10) + 0.5 * det::exp(-age / 0.04))
                            * fade
                            + 0.45 * hug * fade;
                        let mut width = base_w * (1.0 - 0.45 * py::min(age / 0.35, 1.0));
                        let zm = 0.5 * (zz + qz);
                        let (mx, my) = (0.5 * (p.0 + q.0), 0.5 * (p.1 + q.1));
                        let mut front = smooth(-3.0, 3.0, zm);
                        if zm.abs() < CONTACT {
                            front = 0.0;
                            let s_px = pl.sdf(mx, my);
                            width = py::max(2.5, py::min(width, s_px - py::min(2.0, 0.3 * s_px)));
                        }
                        if front < 1.0 && off_block(mx, my) < HW + 4.5 {
                            front = 1.0;
                        }
                        let ob = off_block(mx, my);
                        if ob < 3.0 {
                            prev = Some((p, zz));
                            continue;
                        }
                        weight *= Self::block_dim(mx, my, front);
                        let p_age = 0.06 + 0.045 * age;
                        let e = 1.0 - p_age / 0.55;
                        let env = e * e;
                        let tl = py::rem(t, LOOP);
                        let light = pl.gain * gain * weight / env * (1.0 + self.surge(tl));
                        let young = py::max(0.0, 1.0 - age / 0.35);
                        let ions = 0.6 * if self.in_surge(tl) { 2.0 } else { 1.0 };
                        out.push(fmt(
                            &[
                                front,
                                light,
                                p.0,
                                p.1,
                                q.0,
                                q.1,
                                width,
                                0.7 * P_UNIT,
                                p_age,
                                age,
                                0.12 * young * young,
                                1.4,
                                5.0,
                                ions,
                            ],
                            0xE7_0000 + (strand * nch + kk) % 65536,
                        ));
                    }
                }
                prev = Some((p, zz));
            }
        }
    }

    fn frame_lines(&self, pl: &Plasma, t_ms2: i64) -> Vec<String> {
        let t = fl(t_ms2 % 20000) / 2000.0;
        let mut out = Vec::new();
        self.residual(pl, t, &mut out);
        self.tongues(pl, t, &mut out);
        // the head itself, the newest 40 ms, then the rope the flow carries
        self.chain_chords(pl, t_ms2, &mut out, 0.05);
        self.parcel_rope(pl, t_ms2, &mut out);
        out
    }

    /// The chain's events in the loop: its crossings, wall hits, grazes and
    /// surges, seconds.
    fn events(&self) -> Vec<f64> {
        let mut e: Vec<f64> = self.ch.cross.iter().map(|c| c.0).collect();
        e.extend(self.ch.walls.iter().map(|w| w.t));
        e.extend(self.ch.grazes.iter().map(|g| g.t0));
        e.extend(&self.surges);
        e
    }

    /// The film's start in the loop, cs: of the starts on a blink cycle,
    /// the one with the most events between its fades, then the least light
    /// under them.
    fn choose_window(&self, light: &[(i64, f64)]) -> (i64, usize) {
        let events = self.events();
        let mut best: Option<((usize, f64, i64), i64)> = None;
        for start in (0..1000).step_by(50) {
            let n = events
                .iter()
                .filter(|&&e| {
                    let d = py::rem(e - fl(start) / 100.0, LOOP);
                    FADE_IN <= d && d < fl(FILM_CS) / 100.0 - FADE_OUT
                })
                .count();
            let ends = light.iter().filter(|(cs, _)| {
                let d = fl((cs - start).rem_euclid(1000));
                d < FADE_IN * 100.0 || d >= fl(FILM_CS) - FADE_OUT * 100.0
            });
            let under = py::fsum(
                ends.filter(|(cs, _)| (cs - start).rem_euclid(1000) < FILM_CS)
                    .map(|&(_, l)| l),
            );
            let key = (n, -under, -start);
            let better = match &best {
                None => true,
                Some((b, _)) => {
                    key.0 > b.0 || (key.0 == b.0 && (key.1 > b.1 || (key.1 == b.1 && key.2 > b.2)))
                }
            };
            if better {
                best = Some((key, start));
            }
        }
        let (key, start) = best.expect("twenty starts");
        (start, key.0)
    }
}

/// The plasma's fade, `t` seconds into the film.
fn envelope(t: f64) -> f64 {
    let end = fl(FILM_CS) / 100.0;
    smooth(0.0, FADE_IN, t) * (1.0 - smooth(end - FADE_OUT, end, t))
}

/// The frames of the 250 ms phase from `a` cs: every 20 ms, the last 30.
fn phase_times_cs(a: i64) -> Vec<i64> {
    let (mut out, mut tt) = (Vec::new(), a);
    while tt < a + 25 {
        out.push(tt);
        tt += if a + 25 - tt == 3 { 3 } else { 2 };
    }
    out
}

/// The film's frames over the whole loop: in each 250 ms phase, every 30 ms,
/// the last 40.
fn film_times_cs() -> Vec<i64> {
    (0..LOOP_CS)
        .step_by(25)
        .flat_map(|a| (0..8).map(move |i| a + 3 * i))
        .collect()
}

/// A strand line's field `i`, as Python's `float()` reads it.
fn field(line: &str, i: usize) -> f64 {
    line.split(' ')
        .nth(i)
        .expect("a strand's field")
        .parse()
        .expect("a float")
}

/// A frame's light: each strand's light by its width, the residual's
/// flicker left out.
fn light_of(lines: &[String]) -> f64 {
    py::fsum(lines.iter().filter_map(|l| {
        let x: Vec<&str> = l.split(' ').collect();
        (x[8] != "6.0000")
            .then(|| x[2].parse::<f64>().expect("a float") * x[7].parse::<f64>().expect("a float"))
    }))
}

/// A strand's line with its light times `k`.
fn scaled(line: &str, k: f64) -> String {
    let mut x: Vec<String> = line.split(' ').map(str::to_string).collect();
    x[2] = format!("{:.4}", x[2].parse::<f64>().expect("a float") * k);
    x.join(" ")
}

impl Plasma {
    /// The strands file's text and the report's JSON line.
    ///
    /// # Errors
    /// When no chain from the seed meets the targets, or an assertion fails.
    #[allow(clippy::cast_sign_loss, reason = "cs in 0..1000")]
    pub fn strands(&self) -> Result<(String, String), Stop> {
        let b = self.build()?;
        let lp: Vec<i64> = (0..1000).step_by(25).flat_map(phase_times_cs).collect();
        let mut frames: Vec<Option<Vec<String>>> = vec![None; 1000];
        for &cs in &lp {
            frames[cs as usize] = Some(b.frame_lines(self, cs * 20));
        }
        let light: Vec<(i64, f64)> = lp
            .iter()
            .map(|&cs| (cs, light_of(frames[cs as usize].as_ref().expect("a frame"))))
            .collect();
        let (start, n) = b.choose_window(&light);
        let film: Vec<i64> = (0..FILM_CS).step_by(25).flat_map(phase_times_cs).collect();
        let mut text = String::new();
        for (i, &cs) in film.iter().enumerate() {
            let k = envelope(fl(cs) / 100.0);
            writeln!(text, "F {i} {cs}").expect("a String");
            for l in frames[(start + cs).rem_euclid(1000) as usize]
                .as_ref()
                .expect("a frame")
            {
                if field(l, 2) * k >= 1e-4 {
                    text.push_str(&scaled(l, k));
                    text.push('\n');
                }
            }
        }
        writeln!(text, "F {} {FILM_CS}", film.len()).expect("a String");
        let report = format!(
            "{{\"seed\": {}, \"start_cs\": {start}, \"events\": {n}, \"frames\": {}, \"stats\": {}}}\n",
            b.seed,
            film.len() + 1,
            b.stats
        );
        Ok((text, report))
    }

    /// **The film**: the strands file's text for the whole loop, at full
    /// light, and the report's JSON line.
    ///
    /// # Errors
    /// When no chain from the seed meets the targets, or an assertion fails.
    pub fn whole_loop(&self) -> Result<(String, String), Stop> {
        let b = self.build()?;
        let film = film_times_cs();
        let mut text = String::new();
        for (i, &cs) in film.iter().enumerate() {
            writeln!(text, "F {i} {cs}").expect("a String");
            for l in b.frame_lines(self, cs * 20) {
                if field(&l, 2) >= 1e-4 {
                    text.push_str(&l);
                    text.push('\n');
                }
            }
        }
        writeln!(text, "E {LOOP_CS}").expect("a String");
        let report = format!(
            "{{\"seed\": {}, \"period_cs\": {LOOP_CS}, \"events\": {}, \"frames\": {}, \"stats\": {}}}\n",
            b.seed,
            b.events().len(),
            film.len(),
            b.stats
        );
        Ok((text, report))
    }

    /// **The first frame time of the film, in hundredths, whose strands are
    /// not those of the same time a whole loop later**, `None` where every
    /// one repeats: the wrap draws what the next frame in the loop draws.
    ///
    /// # Errors
    /// As [`Plasma::whole_loop`].
    pub fn first_unrepeated(&self) -> Result<Option<i64>, Stop> {
        let b = self.build()?;
        Ok(film_times_cs()
            .into_iter()
            .find(|&cs| b.frame_lines(self, cs * 20) != b.frame_lines(self, (cs + LOOP_CS) * 20)))
    }
}

/// `plasma <geometry dir> <dark|light> <on_ms> <off_ms> <out>`: writes the
/// strands to `out`; the report for stdout.
///
/// # Errors
/// The refusal's text, Python's.
pub fn run(args: &[String]) -> Result<String, Stop> {
    if args.len() != 5 || !matches!(args[1].as_str(), "dark" | "light") {
        return Err(Stop::Exit(
            "usage: baud-pictures plasma <geometry dir> <dark|light> <on_ms> <off_ms> <out>".into(),
        ));
    }
    if (args[2].as_str(), args[3].as_str()) != ("250", "250") {
        return Err(Stop::Exit(format!(
            "plasma: the frame grid is built on 250 ms phases, not {} and {}",
            args[2], args[3]
        )));
    }
    let pl = Plasma::load(Path::new(&args[0]), args[1] == "dark")?;
    let (text, report) = pl.whole_loop()?;
    std::fs::write(&args[4], text).map_err(|e| Stop::Exit(format!("plasma: {}: {e}", args[4])))?;
    Ok(report)
}
