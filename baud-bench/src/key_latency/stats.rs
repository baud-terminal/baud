//! The folds a sweep reports: per-leg statistics, the A/A floor, the arm
//! deltas, and how the rounds and the gap are arranged.

use super::parse::{Count, Ns};
use super::py;
use super::rows::{Diag, PerLeg, Row, Tally, is_completed, legs_of, shared_frame_count};
use super::rows::{LEG_COUNT, SHARED_FRAME};

/// The median, quartiles, extremes and size of one leg.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LegStats {
    /// `statistics.median`.
    pub median: f64,
    /// The first quartile, inclusive.
    pub q1: f64,
    /// The third quartile, inclusive.
    pub q3: f64,
    /// The smallest value.
    pub min: f64,
    /// The largest value.
    pub max: f64,
    /// How many values.
    pub n: Count,
}

impl LegStats {
    /// `repr` of it as the dict the self-test prints.
    pub fn repr(&self) -> String {
        format!(
            "{{'median': {}, 'q1': {}, 'q3': {}, 'min': {}, 'max': {}, 'n': {}}}",
            py::repr_float(self.median),
            py::repr_float(self.q1),
            py::repr_float(self.q3),
            py::repr_float(self.min),
            py::repr_float(self.max),
            self.n,
        )
    }
}

/// [`LegStats`] of a list of values, or `None` if it is empty. Quartiles are
/// `statistics.quantiles(n=4, method="inclusive")`, so they lie within the
/// data and interpolate in that formula's order of operations; a median of
/// an even count is the mean of the middle two.
pub fn leg_stats(values: &[f64]) -> Option<LegStats> {
    if values.is_empty() {
        return None;
    }
    let mut d = values.to_vec();
    d.sort_by(f64::total_cmp);
    let last = d.len() - 1;
    let quartile = |i: usize| -> f64 {
        if d.len() == 1 {
            return d[0];
        }
        let (j, delta) = ((i * last) / 4, (i * last) % 4);
        (d[j] * ((4 - delta) as f64) + d[j + 1] * (delta as f64)) / 4.0
    };
    let mid = d.len() / 2;
    let median = if d.len() % 2 == 1 {
        d[mid]
    } else {
        (d[mid - 1] + d[mid]) / 2.0
    };
    Some(LegStats {
        median,
        q1: quartile(1),
        q3: quartile(3),
        min: d[0],
        max: d[last],
        n: Count(d.len()),
    })
}

/// Paired rows, over one round or many, folded.
#[derive(Clone, PartialEq, Debug)]
pub struct Summary {
    /// The helper's count when a diag is given (it can exceed `paired` when
    /// lines were lost), else the rows.
    pub posted: Count,
    /// The rows.
    pub paired: Count,
    /// Rows that were not excluded and reached vsync.
    pub completed: Count,
    /// Rows excluded.
    pub excluded: Count,
    /// Excluded rows, by reason.
    pub reasons: Tally,
    /// Keys that shared a frame.
    pub shared: Count,
    /// Each leg's statistics.
    pub legs: PerLeg<Option<LegStats>>,
}

/// Fold rows into a [`Summary`].
pub fn summarize(rows: &[Row], diag: Option<&Diag>) -> Summary {
    let mut reasons = Tally::default();
    for r in rows {
        if let Some(why) = &r.exclude_reason {
            let key = if why.starts_with("negative") {
                "negative"
            } else {
                why
            };
            reasons.add(key, Count(1));
        }
    }
    let mut leg_vals: [Vec<f64>; LEG_COUNT] = std::array::from_fn(|_| Vec::new());
    for r in rows {
        let legs = legs_of(r);
        for (vals, v) in leg_vals.iter_mut().zip(legs.0) {
            vals.extend(v);
        }
    }
    Summary {
        posted: diag.map_or(Count(rows.len()), |d| d.posted),
        paired: Count(rows.len()),
        completed: Count(rows.iter().filter(|r| is_completed(r)).count()),
        excluded: Count(rows.iter().filter(|r| r.excluded()).count()),
        reasons,
        shared: shared_frame_count(rows, SHARED_FRAME),
        legs: PerLeg(std::array::from_fn(|i| leg_stats(&leg_vals[i]))),
    }
}

/// The runs' counters summed, the `refused` lines by why. `None` when no run
/// counted anything: one that never presented has no diag at all.
pub fn merge_diags(diags: &[Option<Diag>]) -> Option<Diag> {
    let mut merged: Option<Diag> = None;
    for d in diags.iter().flatten() {
        let m = merged.get_or_insert_with(Diag::default);
        m.posted += d.posted;
        m.keylats += d.keylats;
        m.paired += d.paired;
        m.unpaired_posts += d.unpaired_posts;
        m.unpaired_keylats += d.unpaired_keylats;
        m.seq_mismatch += d.seq_mismatch;
        m.event_before_post += d.event_before_post;
        m.window += d.window;
        m.negative += d.negative;
        for (sum, one) in [
            (&mut m.parse_failures, d.parse_failures),
            (&mut m.landing_refused, d.landing_refused),
            (&mut m.refused_landing_rows, d.refused_landing_rows),
        ] {
            if let Some(one) = one {
                *sum.get_or_insert(Count(0)) += one;
            }
        }
        for (k, v) in &d.predict_refused.0 {
            m.predict_refused.add(k, *v);
        }
    }
    merged
}

/// One leg's A/A noise floor and what set it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Floor {
    /// The two medians' difference.
    pub dmed: f64,
    /// The base arm's interquartile range.
    pub iqr_base: f64,
    /// The A/A arm's interquartile range.
    pub iqr_aa: f64,
    /// The largest of the three.
    pub floor: f64,
}

/// Per leg, the A/A noise floor: the larger of |median(base) - median(aa)|
/// and either arm's interquartile range. Never max-min, which one fat-tail
/// outlier owns. `None` for a leg one arm has no value for.
pub fn aa_floor(
    base: &PerLeg<Option<LegStats>>,
    aa: &PerLeg<Option<LegStats>>,
) -> PerLeg<Option<Floor>> {
    PerLeg(std::array::from_fn(|i| {
        let (b, a) = (base.0[i]?, aa.0[i]?);
        let dmed = (b.median - a.median).abs();
        let (iqr_base, iqr_aa) = (b.q3 - b.q1, a.q3 - a.q1);
        Some(Floor {
            dmed,
            iqr_base,
            iqr_aa,
            floor: dmed.max(iqr_base).max(iqr_aa),
        })
    }))
}

/// Whether two arms' interquartile ranges overlap.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Iqrs {
    /// They do not.
    Disjoint,
    /// They do.
    Overlap,
}

/// Whether the two interquartile ranges overlap.
pub fn iqr_disjoint(b: &LegStats, a: &LegStats) -> Iqrs {
    if b.q3 < a.q1 || a.q3 < b.q1 {
        Iqrs::Disjoint
    } else {
        Iqrs::Overlap
    }
}

/// One leg's move against its floor.
#[derive(Clone, PartialEq, Debug)]
pub struct Delta {
    /// `median(arm) - median(base)`.
    pub delta: f64,
    /// Whether the arms' interquartile ranges overlap.
    pub iqrs: Iqrs,
    /// The A/A floor, when there is one.
    pub floor: Option<f64>,
    /// What the row says.
    pub verdict: String,
}

/// Per leg, `median(arm) - median(base)` against the A/A floor: the verdict
/// is `result` only when |delta| exceeds the floor AND the two arms'
/// interquartile ranges are disjoint; otherwise within the floor, or the
/// ranges overlap, or there is no floor. `None` for a leg one arm has no
/// value for.
pub fn arm_deltas(
    base: &PerLeg<Option<LegStats>>,
    arm: &PerLeg<Option<LegStats>>,
    floors: Option<&PerLeg<Option<Floor>>>,
) -> Vec<Option<Delta>> {
    (0..LEG_COUNT)
        .map(|i| {
            let (b, a) = (base.0[i]?, arm.0[i]?);
            let delta = a.median - b.median;
            let iqrs = iqr_disjoint(&b, &a);
            let floor = floors.and_then(|f| f.0[i]).map(|f| f.floor);
            let verdict = match floor {
                None => "no A/A floor -- not a result".to_string(),
                Some(floor) if delta.abs() <= floor => format!(
                    "within floor -- not a result (|delta| {:.1} <= floor {floor:.1})",
                    delta.abs()
                ),
                Some(_) if iqrs == Iqrs::Overlap => "IQRs overlap -- not a result".to_string(),
                Some(_) => "result".to_string(),
            };
            Some(Delta {
                delta,
                iqrs,
                floor,
                verdict,
            })
        })
        .collect()
}

/// A round of a sweep, from 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Round(pub usize);

/// The arm order for a round: rotated left by `rnd - 1`, so every arm takes
/// every position over a sweep and none is always last.
pub fn rotate<T: Clone>(items: &[T], rnd: Round) -> Vec<T> {
    if items.is_empty() {
        return Vec::new();
    }
    let mut out = items.to_vec();
    out.rotate_left((rnd.0 - 1) % items.len());
    out
}

/// Whether the warm-up round is among the rounds a sweep pools.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WarmUp {
    /// Round 1 was discarded.
    Discarded,
    /// There was only round 1, which the report must say.
    Used,
}

/// Round 1 is warm-up and discarded. With a single round there is nothing
/// else, and the caller must say so.
pub fn settled_rounds<T>(rounds: &[T]) -> (&[T], WarmUp) {
    if rounds.len() > 1 {
        (&rounds[1..], WarmUp::Discarded)
    } else {
        (rounds, WarmUp::Used)
    }
}

/// True when there is a discarded warm-up round (more than one round) and it
/// completed zero keys -- a launch that never worked in round 1 must be said,
/// not absorbed. A single round is used, not discarded, and
/// [`settled_rounds`] flags it instead.
pub fn warmup_measured_nothing(rounds: &[Vec<Row>]) -> bool {
    rounds.len() > 1 && !rounds[0].iter().any(is_completed)
}

/// (arm, round, count) for every run whose app printed `keylat: landing
/// refused`, in arm then round order.
pub fn refused_runs(per_arm: &[(String, Vec<Option<Diag>>)]) -> Vec<(String, Round, Count)> {
    let mut out = Vec::new();
    for (name, diags) in per_arm {
        for (i, d) in diags.iter().enumerate() {
            let k = d
                .as_ref()
                .and_then(|d| d.landing_refused)
                .unwrap_or(Count(0));
            if k != Count(0) {
                out.push((name.clone(), Round(i + 1), k));
            }
        }
    }
    out
}

/// A span in whole milliseconds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ms(pub i128);

/// The share of the mean gap the helper draws each inter-key gap around,
/// uniformly (key-post.swift, `gapSchedule`).
pub const GAP_JITTER: f64 = 0.25;

/// The pairing window above any gap whose minimum reaches it.
pub const DEFAULT_WINDOW: Ms = Ms(100);

/// Below that, the window is the minimum gap less this, so a pairing shifted
/// by one key cannot fit inside it.
pub const WINDOW_MARGIN: Ms = Ms(5);

/// And it is never under this.
pub const WINDOW_FLOOR: Ms = Ms(1);

/// The range the helper draws each inter-key gap from, in milliseconds:
/// `gap -/+ gap * GAP_JITTER`; a gap of 0 stays (0, 0).
pub fn gap_bounds(gap: Ms) -> (f64, f64) {
    let g = gap.0 as f64;
    let q = g * GAP_JITTER;
    (g - q, g + q)
}

/// The pairing window: 100 ms, or the MINIMUM jittered gap less 5 when that
/// minimum is under 100 ms, floored at 1 (a gap of a few ms cannot separate
/// two keys by time at all; the seq check is the only detector left there).
/// It follows the minimum, not the mean, so a pairing shifted by one key
/// cannot fit inside it however the jitter fell.
pub fn window_for_gap(gap: Ms) -> Ms {
    let (lo, _) = gap_bounds(gap);
    if lo >= DEFAULT_WINDOW.0 as f64 {
        return DEFAULT_WINDOW;
    }
    Ms(((lo - WINDOW_MARGIN.0 as f64) as i128).max(WINDOW_FLOOR.0))
}

/// That window in nanoseconds.
pub fn window_ns(window: Ms) -> Ns {
    Ns(window.0 * 1_000_000)
}
