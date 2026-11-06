//! **The partial bake: `wght` pinned, every other axis kept.** What
//! [`bake_partial`](super::bake_partial) writes is what fontTools 4.63.0's
//! `instantiateVariableFont(font, {"wght": w})` writes with its defaults
//! (`[[source: fonttools-4-63-0-instancer]]`), held to it table by table by
//! `tools/baud-fontbuild`'s comparator over Noto Sans Devanagari at 400 and
//! 700 (`tools/baud-fontbuild/tests/bake.rs`). The rules, read at source:
//!
//! * a tuple's or a region's tent on `wght` reduces to its scalar at the
//!   pinned position: `rebaseTent` on a pinned axis answers
//!   `[(supportScalar, None)]` (`[[source: fonttools-4-63-0-solver]]`), a
//!   tent that peaks at 0 is left alone, and an ill-formed one is dropped
//!   (`changeTupleVariationAxisLimit`); a store's region takes part on an
//!   axis only where its peak is not 0 (`[[source: fonttools-4-63-0-varstore]]`);
//! * a gvar tuple is scaled, then inferred against the default outline,
//!   then summed with every tuple whose kept axes have the same tent, in
//!   order; the ones that keep no axis move the default outline, the rest
//!   are rounded with `otRound` (`instantiateTupleVariationStore`,
//!   `[[source: fonttools-4-63-0-tuplevariation]]`);
//! * each kept tuple is then thinned by `iup_delta_optimize` at a tolerance
//!   of 0.5 against the moved outline rounded, and the thinned form is kept
//!   only when `TupleVariation.compile` makes it smaller
//!   (`_instantiateGvarGlyph`, `TupleVariation.optimize`,
//!   `[[source: fonttools-4-63-0-iup]]`). The comparator reads every
//!   inferred delta, so the explicit points are fontTools' exactly;
//! * an item variation store's columns are scaled and summed the same way,
//!   per data set, without inference; GPOS anchors and value records add
//!   their default delta, rounded, and keep their device
//!   (`instantiateItemVariationStore`, `MutatorMerger`,
//!   `[[source: fonttools-4-63-0-merger]]`);
//! * `fvar` loses the axis and every named instance not at the pinned
//!   value, `avar` the axis's map, `STAT` the axis values the pin
//!   excludes, and `name` the records only the dropped ones named
//!   (`instantiateFvar`, `instantiateAvar`, `instantiateSTAT`,
//!   `[[source: fonttools-4-63-0-instancer-names]]`'s `pruningUnusedNames`);
//!   `head.modified` is the caller's, as fontTools stamps it from
//!   `SOURCE_DATE_EPOCH` on save.
//!
//! **Where it writes something else, the comparator does not read it**:
//! item stores keep every data set and row of the input, each row's deltas
//! fontTools' (it then reorders and merges them with `VarStore.optimize`,
//! and a row is read through its index either way), gvar carries no shared
//! tuples or shared points, and the old stores of `GDEF` and `HVAR` stay in
//! their tables, named by nothing.
//!
//! **What it refuses**, beyond what the full bake refuses: a pin on an
//! axis other than `wght`, a face of one axis (that is the full bake) or
//! more than [`MAX_AXES`], a glyph of more than [`MAX_TUPLES`] tuples or a
//! tuple naming more than 2,048 points, a null data set, a gvar delta past
//! 16 bits once summed, an `avar` map fontTools would drop, a format-1
//! `name` table, a `GDEF` with a caret list, a device on a value its record
//! does not carry unless its delta is 0, and a store that keeps no region
//! (fontTools downgrades `GDEF` then, which this does not do).
//!
//! **One rule it cannot take from fontTools exactly**: the tolerance test
//! rests on the C library's `hypot`, which is not the same function on
//! every platform ([`within_half`]). Over Noto's two instances the choice
//! makes no difference.

use super::*;
use crate::ivs::Set;

/// The most axes a face the partial bake takes may have.
pub(super) const MAX_AXES: usize = 4;
/// The most tuples one glyph's variation data may hold here.
pub(super) const MAX_TUPLES: usize = 256;

/// `head.modified`: seconds since 1904-01-01 00:00 UTC, the format's
/// `LONGDATETIME`. fontTools stamps it on save; `SOURCE_DATE_EPOCH`
/// 315532800 is 2398377600.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Modified(pub i64);

/// One cell of `iup_delta_optimize`'s dynamic programme: the cheapest
/// encoding of the points up to this one, and the explicit point before it.
#[derive(Clone, Copy, Debug)]
pub struct IupStep {
    cost: i32,
    chain: i32,
}

impl IupStep {
    pub const ZERO: IupStep = IupStep { cost: 0, chain: 0 };
}

/// The chain's end, `None` in fontTools' dictionary.
const NO_CHAIN: i32 = -2;

/// The pin: which axis, out of how many, at what normalised and user
/// value.
#[derive(Clone, Copy)]
pub(super) struct Pin {
    pub(super) axis: Index,
    pub(super) axes: Count,
    pub(super) v: F2Dot14,
    pub(super) user: Fixed,
    pub(super) modified: Modified,
}

/// A tuple's or a region's tents on the kept axes, in `fvar` order with the
/// pinned axis left out; an axis that does not take part is all zeros.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Key([[F2Dot14; 3]; MAX_AXES]);

impl Key {
    const DEFAULT: Key = Key([[F2Dot14::ZERO; 3]; MAX_AXES]);
}

/// What the pin does to a tuple or a column.
#[derive(Clone, Copy)]
enum Class {
    /// Dropped: an ill-formed tent, or a scalar of 0.
    Drop,
    /// It keeps no axis: scaled, it moves the default.
    Default(f64),
    /// It keeps the tents of `Key`, scaled.
    Kept(Key, f64),
}

/// fontTools' `scaleDeltas`, which leaves a delta alone at 1.
fn scaled(d: f64, s: f64) -> f64 {
    if s == 1.0 { d } else { d * s }
}

/// The scalar on the pinned axis and the kept tents. `region` names an
/// axis's (start, peak, end); `takes_part` is the rule of the reader that
/// built the dictionary: gvar's keeps a region that is not all zeros,
/// `VarRegion.get_support` one whose peak is not 0.
fn classify(
    pin: Pin,
    region: impl Fn(Index) -> Option<[F2Dot14; 3]>,
    takes_part: impl Fn([F2Dot14; 3]) -> Option<()>,
) -> Option<Class> {
    let [l, p, u] = region(pin.axis)?;
    let Some(s) = tent(l, p, u, pin.v) else {
        return Some(Class::Drop);
    };
    if s == 0.0 {
        return Some(Class::Drop);
    }
    let mut key = Key::DEFAULT;
    let mut k = 0usize;
    let mut a = Index::ZERO;
    let mut kept = false;
    while pin.axes.holds(a) {
        if a != pin.axis {
            let t = region(a)?;
            if takes_part(t).is_some() {
                *key.0.get_mut(k)? = t;
                kept = true;
            }
            k += 1;
        }
        a = a.next();
    }
    Some(if kept {
        Class::Kept(key, s)
    } else {
        Class::Default(s)
    })
}

/// A gvar tuple's tent on axis `a`, the region inferred from the peak when
/// the tuple carries none (`inferRegion_`).
fn tuple_tent(t: &TupleData<'_>, a: Index) -> [F2Dot14; 3] {
    let peak = t.peak.at(a);
    match t.region {
        Some((s, e)) => [s.at(a), peak, e.at(a)],
        None => [F2Dot14(peak.0.min(0)), peak, F2Dot14(peak.0.max(0))],
    }
}

fn tuple_class(t: &TupleData<'_>, pin: Pin) -> Option<Class> {
    classify(
        pin,
        |a| Some(tuple_tent(t, a)),
        |r| (r != [F2Dot14::ZERO; 3]).then_some(()),
    )
}

fn region_class(store: &ItemStore<'_>, r: Index, pin: Pin) -> Option<Class> {
    classify(
        pin,
        |a| store.region(r, a),
        |t| (t[1] != F2Dot14::ZERO).then_some(()),
    )
}

/// One tuple's deltas, scaled by `s`, in `pts.dx`/`dy`, the points it
/// leaves out inferred from the default outline -- the full bake's walk.
fn load(mut t: TupleData<'_>, s: f64, pts: &mut [BakePoint]) -> Option<()> {
    for p in pts.iter_mut() {
        p.dx = 0.0;
        p.dy = 0.0;
        p.given = Given::No;
    }
    let mut list = t.points.map(|p: Points<'_>| p.iter());
    let mut k = Count::ZERO;
    while k != t.n {
        let i = match list.as_mut() {
            Some(it) => Points::next_from(it),
            None => k.as_index(),
        };
        k = k.plus(Count::of(1u32))?;
        let (x, y) = (t.xs.next(), t.ys.next());
        let p = pts.get_mut(i.slot())?;
        p.dx = scaled(f64::from(x), s);
        p.dy = scaled(f64::from(y), s);
        p.given = Given::Yes;
    }
    if t.points.is_some() {
        infer(pts);
    }
    Some(())
}

/// The sum of the tuples that keep no axis, in `sx`/`sy`.
fn default_sum(store: Option<Tuples<'_>>, pin: Pin, pts: &mut [BakePoint]) -> Option<()> {
    for p in pts.iter_mut() {
        p.sx = 0.0;
        p.sy = 0.0;
    }
    let Some(store) = store else {
        return Some(());
    };
    let total = Count::of(u32::try_from(pts.len()).ok()?);
    store.tuples(total, |t| {
        if let Class::Default(s) = tuple_class(&t, pin)? {
            load(t, s, pts)?;
            for p in pts.iter_mut() {
                p.sx += p.dx;
                p.sy += p.dy;
            }
        }
        Some(())
    })
}

/// The default outline moved by the tuples that keep no axis.
pub(super) fn vary_default(
    store: Option<Tuples<'_>>,
    pin: Pin,
    pts: &mut [BakePoint],
) -> Option<()> {
    default_sum(store, pin, pts)?;
    for p in pts.iter_mut() {
        p.x += p.sx;
        p.y += p.sy;
    }
    Some(())
}

/// `abs(complex(a, b)) <= 0.5`, fontTools' test of a delta against its
/// tolerance, as `sqrt(a * a + b * b) <= 0.5` in `f64`. CPython's `abs`
/// calls the C library's `hypot`, and the answer is the platform's within
/// about an ulp of 0.5: this Mac's gives 0.5 for (0.40000000000000013,
/// 0.29999999999999993), where correct rounding gives the next double, and
/// no threshold on `a*a + b*b` reproduces it. This form is deterministic
/// everywhere and agreed with it on every case of a million random contours
/// and on both Noto instances; the pinned hashes hold the fonts themselves.
fn within_half(a: f64, b: f64) -> bool {
    core::f64::math::sqrt(a * a + b * b) <= 0.5
}

/// One contour's points as the optimiser reads them: the rounded moved
/// position and the rounded summed delta.
#[derive(Clone, Copy)]
struct Opt {
    rx: f64,
    ry: f64,
    dx: f64,
    dy: f64,
}

fn opt(p: &BakePoint) -> Opt {
    Opt {
        rx: ot_round(p.x + p.sx),
        ry: ot_round(p.y + p.sy),
        dx: p.ax,
        dy: p.ay,
    }
}

/// fontTools' test of a gap: whether the points strictly between working
/// positions `i` and `j` (`i` may be -1, the list's last) infer within the
/// tolerance from those two.
fn can_iup(at: &impl Fn(i64) -> Opt, len: i64, i: i64, j: i64) -> bool {
    let a = at(if i < 0 { len - 1 } else { i });
    let b = at(j);
    let mut m = i + 1;
    while m < j {
        let p = at(m);
        let ix = iup(p.rx, (a.rx, a.dx), (b.rx, b.dx));
        let iy = iup(p.ry, (a.ry, a.dy), (b.ry, b.dy));
        if !within_half(p.dx - ix, p.dy - iy) {
            return false;
        }
        m += 1;
    }
    true
}

/// fontTools' dynamic programme over `len` working positions, the cell of
/// position `i` at `steps[i + 1]`.
fn dp(
    at: &impl Fn(i64) -> Opt,
    forced: &impl Fn(i64) -> bool,
    len: i64,
    lookback: i64,
    steps: &mut [IupStep],
) -> Option<()> {
    let cell = |i: i64| usize::try_from(i + 1).ok();
    *steps.get_mut(0)? = IupStep {
        cost: 0,
        chain: NO_CHAIN,
    };
    let lookback = lookback.min(8);
    for i in 0..len {
        let prev = steps.get(cell(i - 1)?)?.cost;
        let mut best = prev + 1;
        *steps.get_mut(cell(i)?)? = IupStep {
            cost: best,
            chain: i32::try_from(i - 1).ok()?,
        };
        if i >= 1 && forced(i - 1) {
            continue;
        }
        let stop = (i - lookback).max(-2);
        let mut j = i - 2;
        while j > stop {
            let cost = steps.get(cell(j)?)?.cost + 1;
            if cost < best && can_iup(at, len, j, i) {
                best = cost;
                *steps.get_mut(cell(i)?)? = IupStep {
                    cost,
                    chain: i32::try_from(j).ok()?,
                };
            }
            if j >= 0 && forced(j) {
                break;
            }
            j -= 1;
        }
    }
    Some(())
}

/// fontTools' forced set, at a tolerance of 0.5, into each
/// point's `forced`.
fn bound_forced(c: &mut [BakePoint]) {
    let n = c.len();
    let tol = 0.5;
    for i in (0..n).rev() {
        let (l, d, nx) = (opt(&c[(i + n - 1) % n]), opt(&c[i]), opt(&c[(i + 1) % n]));
        let mut force = Forced::No;
        for (cj, dj, lcj, ldj, ncj, ndj) in [
            (d.rx, d.dx, l.rx, l.dx, nx.rx, nx.dx),
            (d.ry, d.dy, l.ry, l.dy, nx.ry, nx.dy),
        ] {
            let (c1, c2, d1, d2) = if lcj <= ncj {
                (lcj, ncj, ldj, ndj)
            } else {
                (ncj, lcj, ndj, ldj)
            };
            let hit = if c1 == c2 {
                (d1 - d2).abs() > tol && dj.abs() > tol
            } else if c1 <= cj && cj <= c2 {
                !(d1.min(d2) - tol <= dj && dj <= d1.max(d2) + tol)
            } else if d1 != d2 {
                if cj < c1 {
                    dj.abs() > tol && (dj - d1).abs() > tol && ((dj - tol < d1) != (d1 < d2))
                } else {
                    dj.abs() > tol && (dj - d2).abs() > tol && ((d2 < dj + tol) != (d1 < d2))
                }
            } else {
                false
            };
            if hit {
                force = Forced::Yes;
                break;
            }
        }
        c[i].forced = force;
    }
}

/// `iup_contour_optimize` at 0.5: each point of the contour marked
/// explicit or implied.
fn optimize_contour(c: &mut [BakePoint], steps: &mut [IupStep]) -> Result<(), Refused> {
    let n = c.len();
    let mark = |c: &mut [BakePoint], f: &dyn Fn(usize) -> Choice| {
        for (i, p) in c.iter_mut().enumerate() {
            p.choice = f(i);
        }
    };
    if c.iter().all(|p| within_half(p.ax, p.ay)) {
        mark(c, &|_| Choice::Implied);
        return Ok(());
    }
    if n == 1 {
        mark(c, &|_| Choice::Explicit);
        return Ok(());
    }
    let (fx, fy) = (c[0].ax, c[0].ay);
    if c.iter().all(|p| p.ax == fx && p.ay == fy) {
        mark(c, &|i| {
            if i == 0 {
                Choice::Explicit
            } else {
                Choice::Implied
            }
        });
        return Ok(());
    }
    bound_forced(c);
    let len = i64::try_from(n).map_err(|_| Refused::Scratch)?;
    let need = 2 * n + 1;
    if steps.len() < need {
        return Err(Refused::Scratch);
    }
    let last_forced = c.iter().rposition(|p| p.forced == Forced::Yes);
    // The working list: rotated so the last forced point ends it, or the
    // contour twice over when nothing is forced. Position `m` is contour
    // point `orig(m)`.
    let (k, work) = match last_forced {
        Some(top) => (
            i64::try_from(n - 1 - top).map_err(|_| Refused::Scratch)?,
            len,
        ),
        None => (0, 2 * len),
    };
    let orig = |m: i64| usize::try_from((m - k).rem_euclid(len)).unwrap_or(0);
    {
        let ro: &[BakePoint] = c;
        let at = |m: i64| opt(&ro[orig(m)]);
        let forced = |m: i64| last_forced.is_some() && ro[orig(m)].forced == Forced::Yes;
        dp(&at, &forced, work, len, steps).ok_or(Refused::Scratch)?;
    }
    let cell = |i: i64| usize::try_from(i + 1).map_err(|_| Refused::Scratch);
    let chain = |i: i64| -> Result<i64, Refused> {
        Ok(i64::from(
            steps.get(cell(i)?).ok_or(Refused::Scratch)?.chain,
        ))
    };
    let cost = |i: i64| -> Result<i64, Refused> {
        Ok(i64::from(steps.get(cell(i)?).ok_or(Refused::Scratch)?.cost))
    };
    // The chain to walk: from the last position when something is forced,
    // else from the cheapest start (the last of the cheapest, `<=`).
    let (from, until) = match last_forced {
        Some(_) => (len - 1, -1),
        None => {
            let mut best: Option<i64> = None;
            let mut best_cost = len + 1;
            for start in (len - 1)..(2 * len) {
                let mut i = start;
                while i > start - len {
                    i = chain(i)?;
                }
                if i == start - len {
                    let c = cost(start)? - cost(start - len)?;
                    if c <= best_cost {
                        best = Some(start);
                        best_cost = c;
                    }
                }
            }
            let start = best.ok_or(Refused::Scratch)?;
            (start, start - len)
        }
    };
    for p in c.iter_mut() {
        p.choice = Choice::Implied;
    }
    let mut i = from;
    while i > until {
        c.get_mut(orig(i)).ok_or(Refused::Scratch)?.choice = Choice::Explicit;
        i = chain(i)?;
    }
    Ok(())
}

/// Whether a point is in the forced set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Forced {
    No,
    Yes,
}

/// Whether the optimiser keeps a point's delta or leaves it to inference.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Choice {
    Explicit,
    Implied,
}

/// fontTools' `compilePoints` of the explicit points, into `w`.
fn put_points(pts: &[BakePoint], w: &mut Cursor<'_, '_>) -> Result<(), Refused> {
    let used = pts.iter().filter(|p| p.choice == Choice::Explicit).count();
    let used = u16::try_from(used).map_err(|_| Refused::Metrics)?;
    if used < 0x80 {
        w.put(used.to_be_bytes()[1])?;
    } else {
        w.put(used | 0x8000)?;
    }
    let numbers = pts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.choice == Choice::Explicit)
        .map(|(i, _)| i);
    let mut it = numbers.peekable();
    let mut last = 0usize;
    while it.peek().is_some() {
        let header = w.at;
        w.put(0u8)?;
        let mut run = 0u8;
        let mut bytes: Option<bool> = None;
        while let Some(&cur) = it.peek() {
            if run > 127 {
                break;
            }
            let delta = cur.checked_sub(last).ok_or(Refused::Metrics)?;
            let byte = *bytes.get_or_insert(delta <= 0xFF);
            if byte && delta > 0xFF {
                break;
            }
            if byte {
                w.put(u8::try_from(delta).map_err(|_| Refused::Metrics)?)?;
            } else {
                w.put(u16::try_from(delta).map_err(|_| Refused::Metrics)?)?;
            }
            last = cur;
            it.next();
            run += 1;
        }
        let flag = if bytes == Some(true) { 0 } else { 0x80 };
        w.out.put(header, (run - 1) | flag);
    }
    Ok(())
}

/// fontTools' `compileDeltaValues_` with `optimizeSize`, over the explicit
/// points' deltas on one axis.
fn put_deltas(values: &[i16], w: &mut Cursor<'_, '_>) -> Result<(), Refused> {
    let n = values.len();
    let mut pos = 0usize;
    let byte = |v: i16| (-128..=127).contains(&v);
    while pos < n {
        let v = values[pos];
        let start = pos;
        if v == 0 {
            while pos < n && values[pos] == 0 {
                pos += 1;
            }
            let mut run = pos - start;
            while run >= 64 {
                w.put(0x80u8 | 63)?;
                run -= 64;
            }
            if run > 0 {
                w.put(0x80u8 | u8::try_from(run - 1).map_err(|_| Refused::Metrics)?)?;
            }
        } else if byte(v) {
            while pos < n {
                let x = values[pos];
                if !byte(x) {
                    break;
                }
                if x == 0 && pos + 1 < n && values[pos + 1] == 0 {
                    break;
                }
                pos += 1;
            }
            let mut at = start;
            while pos - at >= 64 {
                w.put(63u8)?;
                for x in &values[at..at + 64] {
                    w.put(x.to_be_bytes()[1])?;
                }
                at += 64;
            }
            if pos > at {
                w.put(u8::try_from(pos - at - 1).map_err(|_| Refused::Metrics)?)?;
                for x in &values[at..pos] {
                    w.put(x.to_be_bytes()[1])?;
                }
            }
        } else {
            while pos < n {
                let x = values[pos];
                if x == 0 {
                    break;
                }
                if byte(x) && pos + 1 < n && byte(values[pos + 1]) {
                    break;
                }
                pos += 1;
            }
            let mut at = start;
            while pos - at >= 64 {
                w.put(0x40u8 | 63)?;
                for x in &values[at..at + 64] {
                    w.put(*x)?;
                }
                at += 64;
            }
            if pos > at {
                w.put(0x40u8 | u8::try_from(pos - at - 1).map_err(|_| Refused::Metrics)?)?;
                for x in &values[at..pos] {
                    w.put(*x)?;
                }
            }
        }
    }
    Ok(())
}

/// The explicit points' deltas on one axis, as 16-bit values, into
/// `values`; a delta past 16 bits is refused.
fn explicit(
    pts: &[BakePoint],
    axis: DeltaAxis,
    every: Every,
    values: &mut [i16],
) -> Result<usize, Refused> {
    let mut k = 0usize;
    for p in pts {
        if every == Every::No && p.choice != Choice::Explicit {
            continue;
        }
        let d = match axis {
            DeltaAxis::X => p.ax,
            DeltaAxis::Y => p.ay,
        };
        *values.get_mut(k).ok_or(Refused::Scratch)? = short(d).ok_or(Refused::Metrics)?;
        k += 1;
    }
    Ok(k)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeltaAxis {
    X,
    Y,
}

/// Whether a tuple names every point (the unthinned form) or its explicit
/// ones.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Every {
    Yes,
    No,
}

/// The serialized data of one tuple -- point numbers, then the x and y
/// deltas -- into `w`: every point (`b"\0"`) or the explicit ones.
fn put_data(pts: &[BakePoint], every: Every, w: &mut Cursor<'_, '_>) -> Result<(), Refused> {
    let mut values = [0i16; 2048];
    match every {
        Every::Yes => w.put(0u8)?,
        Every::No => put_points(pts, w)?,
    }
    for axis in [DeltaAxis::X, DeltaAxis::Y] {
        // A tuple naming more points than this is refused.
        let k = explicit(pts, axis, every, &mut values)?;
        put_deltas(&values[..k], w)?;
    }
    Ok(())
}

/// What a write would take, measured by writing it nowhere.
fn measure(f: impl FnOnce(&mut Cursor<'_, '_>) -> Result<(), Refused>) -> Result<Len, Refused> {
    let mut none: [u8; 0] = [];
    let mut out = Out { buf: &mut none };
    let mut w = Cursor {
        out: &mut out,
        at: Offset::ZERO,
    };
    f(&mut w)?;
    Ok(w.at.since_start())
}

/// One kept group's summed, rounded deltas in `ax`/`ay`: every tuple of
/// key `key`, in order, scaled and inferred.
fn group(store: Tuples<'_>, pin: Pin, key: Key, pts: &mut [BakePoint]) -> Option<()> {
    for p in pts.iter_mut() {
        p.ax = 0.0;
        p.ay = 0.0;
    }
    let total = Count::of(u32::try_from(pts.len()).ok()?);
    store.tuples(total, |t| {
        if let Class::Kept(k, s) = tuple_class(&t, pin)? {
            if k == key {
                load(t, s, pts)?;
                for p in pts.iter_mut() {
                    p.ax += p.dx;
                    p.ay += p.dy;
                }
            }
        }
        Some(())
    })?;
    for p in pts.iter_mut() {
        p.ax = ot_round(p.ax);
        p.ay = ot_round(p.ay);
    }
    Some(())
}

/// The kept group's points thinned: each contour optimised, then the
/// thinned form kept only where it is smaller. Answers how it is written,
/// or `None` when no point is left and the tuple goes.
fn thin(pts: &mut [BakePoint], steps: &mut [IupStep]) -> Result<Option<Every>, Refused> {
    let mut start = 0usize;
    while start < pts.len() {
        let mut end = start;
        while end + 1 < pts.len() && pts[end].last != LastPoint::Yes {
            end += 1;
        }
        optimize_contour(&mut pts[start..=end], steps)?;
        start = end + 1;
    }
    if pts.iter().all(|p| p.choice == Choice::Explicit) {
        return Ok(Some(Every::Yes));
    }
    if pts.iter().all(|p| p.choice == Choice::Implied) {
        return Ok(None);
    }
    let thinned = measure(|w| put_data(pts, Every::No, w))?;
    let whole = measure(|w| put_data(pts, Every::Yes, w))?;
    Ok(Some(if thinned < whole {
        Every::No
    } else {
        Every::Yes
    }))
}

/// The tuple's header past its two leading words: the peak on each kept
/// axis, and the intermediate region when one is needed.
fn put_header(key: Key, pin: Pin, size: Len, w: &mut Cursor<'_, '_>) -> Result<(), Refused> {
    let kept = pin.axes.value().saturating_sub(1);
    let kept = usize::try_from(kept).map_err(|_| Refused::Metrics)?;
    let tents = key.0.get(..kept).ok_or(Refused::Axes)?;
    let inter = tents
        .iter()
        .any(|[s, p, e]| s.0 != p.0.min(0) || e.0 != p.0.max(0));
    w.put(u16::try_from(size.value()).map_err(|_| Refused::Metrics)?)?;
    let flags = 0x8000u16 | 0x2000 | if inter { 0x4000 } else { 0 };
    w.put(flags)?;
    for t in tents {
        w.put(t[1].0)?;
    }
    if inter {
        for t in tents {
            w.put(t[0].0)?;
        }
        for t in tents {
            w.put(t[2].0)?;
        }
    }
    Ok(())
}

/// One glyph's variation data, written at `w`.
fn glyph_data(
    ctx: &Ctx<'_>,
    pin: Pin,
    g: GlyphId,
    scratch: &mut [BakePoint],
    steps: &mut [IupStep],
    w: &mut Cursor<'_, '_>,
) -> Result<(), Refused> {
    let bad = Refused::Glyph(g);
    let Some(store) = ctx.gvar.and_then(|v| v.glyph(g)) else {
        return Ok(());
    };
    let mut budget = Budget(0);
    let p = ctx.visit(g, Depth(0), &mut budget)?;
    let count = ctx.gather(g, p, scratch)?;
    let pts = scratch.get_mut(..count).ok_or(Refused::Scratch)?;
    default_sum(Some(store), pin, pts).ok_or(bad)?;
    let total = Count::of(u32::try_from(count).map_err(|_| bad)?);
    // The kept groups, each at its first tuple, in order.
    let mut keys = [Key::DEFAULT; MAX_TUPLES];
    let mut firsts = 0usize;
    store
        .tuples(total, |t| {
            if let Class::Kept(k, _) = tuple_class(&t, pin)? {
                if !keys[..firsts].contains(&k) {
                    *keys.get_mut(firsts)? = k;
                    firsts += 1;
                }
            }
            Some(())
        })
        .ok_or(bad)?;
    let keys = &keys[..firsts];
    // Which survive, how each is written, and how long its data is.
    let mut plans = [None::<(Every, Len)>; MAX_TUPLES];
    for (k, key) in keys.iter().enumerate() {
        group(store, pin, *key, pts).ok_or(bad)?;
        if let Some(every) = thin(pts, steps)? {
            let len = measure(|w| put_data(pts, every, w))?;
            plans[k] = Some((every, len));
        }
    }
    let live = plans.iter().flatten().count();
    if live == 0 {
        return Ok(());
    }
    let mut headers = Len::of(0);
    for (k, key) in keys.iter().enumerate() {
        if let Some((_, len)) = plans[k] {
            let h = measure(|w| put_header(*key, pin, len, w))?;
            headers = Len::of(headers.value() + h.value());
        }
    }
    w.put(u16::try_from(live).map_err(|_| bad)?)?;
    w.put(u16::try_from(4 + headers.value()).map_err(|_| bad)?)?;
    for (k, key) in keys.iter().enumerate() {
        if let Some((_, len)) = plans[k] {
            put_header(*key, pin, len, w)?;
        }
    }
    for (k, key) in keys.iter().enumerate() {
        if let Some((every, _)) = plans[k] {
            group(store, pin, *key, pts).ok_or(bad)?;
            thin(pts, steps)?;
            put_data(pts, every, w)?;
        }
    }
    Ok(())
}

/// The new `gvar`, at `at`: version 1.0, the kept axes, no shared tuples,
/// 32-bit offsets.
pub(super) fn gvar(
    ctx: &Ctx<'_>,
    pin: Pin,
    scratch: &mut [BakePoint],
    steps: &mut [IupStep],
    out: &mut Out<'_>,
    at: Offset,
) -> Result<Len, Refused> {
    let n = u32::from(ctx.face.glyph_count().value());
    let offsets = at.plus(Len::of(20)).ok_or(Refused::Metrics)?;
    let data = offsets
        .plus(Len::of(4).times(Count::of(n + 1)).ok_or(Refused::Metrics)?)
        .ok_or(Refused::Metrics)?;
    let array = data.since(at).ok_or(Refused::Metrics)?;
    let kept = u16::try_from(pin.axes.value() - 1).map_err(|_| Refused::Metrics)?;
    let mut w = Cursor { out, at };
    w.put(1u16)?;
    w.put(0u16)?;
    w.put(kept)?;
    w.put(0u16)?;
    w.put(array.value())?;
    w.put(u16::try_from(n).map_err(|_| Refused::Metrics)?)?;
    w.put(1u16)?;
    w.put(array.value())?;
    let mut w = Cursor {
        out: w.out,
        at: data,
    };
    for k in 0..n {
        let since = w.at.since(data).ok_or(Refused::Metrics)?;
        w.out.put(
            offsets.plus(Len::of(4 * k)).ok_or(Refused::Metrics)?,
            since.value(),
        );
        let g = GlyphId::from_shaper(u16::try_from(k).map_err(|_| Refused::Metrics)?);
        glyph_data(ctx, pin, g, scratch, steps, &mut w)?;
    }
    let since = w.at.since(data).ok_or(Refused::Metrics)?;
    w.out.put(
        offsets.plus(Len::of(4 * n)).ok_or(Refused::Metrics)?,
        since.value(),
    );
    w.at.since(at).ok_or(Refused::Metrics)
}

/// A store's region `r` once pinned: its new index in the list
/// [`store`] writes, `None` when it keeps no axis. The list holds each
/// kept key once, at its first region.
fn new_region(store: &ItemStore<'_>, r: Index, pin: Pin) -> Option<Option<Index>> {
    let key = match region_class(store, r, pin)? {
        Class::Kept(k, _) => k,
        _ => return Some(None),
    };
    let mut rank = 0u32;
    let mut q = Index::ZERO;
    while q != r {
        if let Class::Kept(k, _) = region_class(store, q, pin)? {
            if k == key {
                return Some(Some(Index::of(rank)));
            }
            if is_first(store, q, pin)? {
                rank += 1;
            }
        }
        q = q.next();
    }
    Some(Some(Index::of(rank)))
}

/// Whether region `r` is the first with its kept key.
fn is_first(store: &ItemStore<'_>, r: Index, pin: Pin) -> Option<bool> {
    let Class::Kept(key, _) = region_class(store, r, pin)? else {
        return Some(false);
    };
    let mut q = Index::ZERO;
    while q != r {
        if let Class::Kept(k, _) = region_class(store, q, pin)? {
            if k == key {
                return Some(false);
            }
        }
        q = q.next();
    }
    Some(true)
}

/// Column `col` of `set` once pinned.
fn column_class(store: &ItemStore<'_>, set: &Set<'_>, col: Index, pin: Pin) -> Option<Class> {
    region_class(store, set.region_of(col)?, pin)
}

/// A data set's new columns: each kept key once, at its first column.
fn is_first_column(
    store: &ItemStore<'_>,
    set: &Set<'_>,
    col: Index,
    pin: Pin,
) -> Option<Option<Key>> {
    let Class::Kept(key, _) = column_class(store, set, col, pin)? else {
        return Some(None);
    };
    let mut q = Index::ZERO;
    while q != col {
        if let Class::Kept(k, _) = column_class(store, set, q, pin)? {
            if k == key {
                return Some(None);
            }
        }
        q = q.next();
    }
    Some(Some(key))
}

/// Item `item`'s summed delta in the new column of `key`, unrounded: the
/// columns of that key, scaled, in order.
fn column_sum(
    store: &ItemStore<'_>,
    set: &Set<'_>,
    item: Index,
    key: Option<Key>,
    pin: Pin,
) -> Option<Option<f64>> {
    let mut sum: Option<f64> = None;
    let mut c = Index::ZERO;
    while set.columns().holds(c) {
        let hit = match (column_class(store, set, c, pin)?, key) {
            (Class::Kept(k, s), Some(want)) if k == want => Some(s),
            (Class::Default(s), None) => Some(s),
            _ => None,
        };
        if let Some(s) = hit {
            let d = scaled(f64::from(set.value(item, c)?), s);
            sum = Some(sum.map_or(d, |a| a + d));
        }
        c = c.next();
    }
    Some(sum)
}

/// The default delta of `index`, unrounded: what the columns that keep no
/// axis add at the pin -- `MutatorMerger` rounds it.
pub(super) fn default_delta(store: &ItemStore<'_>, index: VarIndex, pin: Pin) -> Option<f64> {
    if index.is_none() {
        return Some(0.0);
    }
    let set = store.set(index.outer())?;
    if !set.items().holds(index.inner()) {
        return None;
    }
    Some(column_sum(store, &set, index.inner(), None, pin)?.unwrap_or(0.0))
}

/// The pinned store, at `at`: every data set and row kept, so every index
/// still names its item.
pub(super) fn store(
    store: &ItemStore<'_>,
    pin: Pin,
    out: &mut Out<'_>,
    at: Offset,
) -> Result<Len, Refused> {
    let bad = Refused::Malformed(Tag::GDEF);
    if store.region_axes() != pin.axes {
        return Err(bad);
    }
    let sets = store.sets();
    let kept_axes = Count::of(pin.axes.value() - 1);
    // Header, then the data offsets, then the region list, then the sets.
    let mut w = Cursor { out, at };
    w.put(1u16)?;
    let list_at_field = w.at;
    w.put(0u32)?;
    w.put(u16::try_from(sets.value()).map_err(|_| bad)?)?;
    let offsets = w.at;
    for _ in 0..sets.value() {
        w.put(0u32)?;
    }
    let list = w.at;
    w.out.put(
        list_at_field,
        list.since(at).ok_or(Refused::Metrics)?.value(),
    );
    let mut regions = 0u32;
    let mut r = Index::ZERO;
    while store.region_count().holds(r) {
        if is_first(store, r, pin).ok_or(bad)? {
            regions += 1;
        }
        r = r.next();
    }
    w.put(u16::try_from(kept_axes.value()).map_err(|_| bad)?)?;
    w.put(u16::try_from(regions).map_err(|_| bad)?)?;
    let mut r = Index::ZERO;
    while store.region_count().holds(r) {
        if is_first(store, r, pin).ok_or(bad)? {
            let mut a = Index::ZERO;
            while pin.axes.holds(a) {
                if a != pin.axis {
                    for c in store.region(r, a).ok_or(bad)? {
                        w.put(c.0)?;
                    }
                }
                a = a.next();
            }
        }
        r = r.next();
    }
    let mut o = Index::ZERO;
    while sets.holds(o) {
        let set = store.set(o).ok_or(bad)?;
        w.out.put(
            offsets
                .plus(Len::of(4 * o.value()))
                .ok_or(Refused::Metrics)?,
            w.at.since(at).ok_or(Refused::Metrics)?.value(),
        );
        // The new columns, and whether any needs 32 bits.
        let mut cols = 0u32;
        let mut long = false;
        let mut c = Index::ZERO;
        while set.columns().holds(c) {
            if let Some(key) = is_first_column(store, &set, c, pin).ok_or(bad)? {
                cols += 1;
                let mut i = Index::ZERO;
                while set.items().holds(i) {
                    let v = ot_round(
                        column_sum(store, &set, i, Some(key), pin)
                            .ok_or(bad)?
                            .unwrap_or(0.0),
                    );
                    if short(v).is_none() {
                        long = true;
                    }
                    i = i.next();
                }
            }
            c = c.next();
        }
        w.put(u16::try_from(set.items().value()).map_err(|_| bad)?)?;
        let cols16 = u16::try_from(cols).map_err(|_| bad)?;
        w.put(if long { cols16 | 0x8000 } else { cols16 })?;
        w.put(cols16)?;
        let mut c = Index::ZERO;
        while set.columns().holds(c) {
            if is_first_column(store, &set, c, pin).ok_or(bad)?.is_some() {
                let r = set.region_of(c).ok_or(bad)?;
                let n = new_region(store, r, pin).ok_or(bad)?.ok_or(bad)?;
                w.put(u16::try_from(n.value()).map_err(|_| bad)?)?;
            }
            c = c.next();
        }
        let mut i = Index::ZERO;
        while set.items().holds(i) {
            let mut c = Index::ZERO;
            while set.columns().holds(c) {
                if let Some(key) = is_first_column(store, &set, c, pin).ok_or(bad)? {
                    let v = ot_round(
                        column_sum(store, &set, i, Some(key), pin)
                            .ok_or(bad)?
                            .unwrap_or(0.0),
                    );
                    if long {
                        if !(-2147483648.0..=2147483647.0).contains(&v) {
                            return Err(Refused::Metrics);
                        }
                        w.put(v as i32)?;
                    } else {
                        w.put(short(v).ok_or(Refused::Metrics)?)?;
                    }
                }
                c = c.next();
            }
            i = i.next();
        }
        o = o.next();
    }
    if regions == 0 {
        return Err(Refused::Table(Tag::GDEF));
    }
    w.at.since(at).ok_or(Refused::Metrics)
}

/// A table copied whole with a new store appended, its 32-bit offset at
/// `field` pointed at it.
pub(super) fn with_store(
    table: Bytes<'_>,
    old: Option<ItemStore<'_>>,
    field: Offset,
    pin: Pin,
    out: &mut Out<'_>,
    at: Offset,
) -> Result<Len, Refused> {
    out.copy(at, table.as_slice());
    let Some(old) = old else {
        return Ok(table.len());
    };
    let end = at.plus(table.len()).ok_or(Refused::Metrics)?;
    let n = store(&old, pin, out, end)?;
    out.put(
        at.plus(field.since_start()).ok_or(Refused::Metrics)?,
        table.len().value(),
    );
    table
        .len()
        .value()
        .checked_add(n.value())
        .map(Len::of)
        .ok_or(Refused::Metrics)
}

/// The `GDEF` store, when the table has one (version 1.3, a non-null
/// offset); a `GDEF` with a caret list is refused, its devices unread.
pub(super) fn gdef_store<'g>(gdef: Bytes<'g>) -> Result<Option<ItemStore<'g>>, Refused> {
    let bad = Refused::Malformed(Tag::GDEF);
    let minor: u16 = gdef.read(Offset::of(2u32)).ok_or(bad)?;
    let carets: u16 = gdef.read(Offset::of(8u32)).ok_or(bad)?;
    if carets != 0 {
        return Err(Refused::Table(Tag::GDEF));
    }
    if minor < 3 {
        return Ok(None);
    }
    let at: u32 = gdef.read(Offset::of(14u32)).ok_or(bad)?;
    if at == 0 {
        return Ok(None);
    }
    Ok(Some(
        ItemStore::parse(gdef.tail(Offset::of(at)).ok_or(bad)?).ok_or(bad)?,
    ))
}

/// The `HVAR` store.
pub(super) fn hvar_store<'h>(hvar: Bytes<'h>) -> Result<Option<ItemStore<'h>>, Refused> {
    let bad = Refused::Malformed(Tag::HVAR);
    let at: u32 = hvar.read(Offset::of(4u32)).ok_or(bad)?;
    if at == 0 {
        return Ok(None);
    }
    Ok(Some(
        ItemStore::parse(hvar.tail(Offset::of(at)).ok_or(bad)?).ok_or(bad)?,
    ))
}

/// `fvar`'s header fields.
struct FvarHead {
    axes_at: Offset,
    axes: Count,
    axis_size: Len,
    instances: Count,
    instance_size: Len,
}

fn fvar_head(fvar: Bytes<'_>) -> Result<FvarHead, Refused> {
    let bad = Refused::Malformed(Tag::FVAR);
    let r16 = |k: u32| fvar.read::<u16>(Offset::of(k)).ok_or(bad);
    Ok(FvarHead {
        axes_at: Offset::of(r16(4)?),
        axes: Count::of(r16(8)?),
        axis_size: Len::of(u32::from(r16(10)?)),
        instances: Count::of(r16(12)?),
        instance_size: Len::of(u32::from(r16(14)?)),
    })
}

/// Instance `i`'s record: its offset in `fvar`.
fn instance_at(h: &FvarHead, i: Index) -> Option<Offset> {
    let axes = h.axis_size.times(h.axes)?;
    h.axes_at
        .plus(axes)?
        .plus(Offset::ZERO.at(i, h.instance_size)?.since_start())
}

/// Whether instance `i` sits at the pin.
fn at_pin(fvar: Bytes<'_>, h: &FvarHead, i: Index, pin: Pin) -> Result<bool, Refused> {
    let bad = Refused::Malformed(Tag::FVAR);
    let rec = instance_at(h, i).ok_or(bad)?;
    let c = rec
        .plus(Len::of(4))
        .and_then(|o| o.plus(Offset::ZERO.at(pin.axis, Len::of(4))?.since_start()))
        .ok_or(bad)?;
    let v: Fixed = fvar.read(c).ok_or(bad)?;
    Ok(v == pin.user)
}

/// Whether the instance records carry `postScriptNameID`.
fn has_ps_name(h: &FvarHead) -> Result<bool, Refused> {
    let base = 4 + 4 * h.axes.value();
    match h.instance_size.value() {
        s if s == base => Ok(false),
        s if s == base + 2 => Ok(true),
        _ => Err(Refused::Malformed(Tag::FVAR)),
    }
}

/// The new `fvar`: the pinned axis gone, the instances at the pin kept
/// without it.
pub(super) fn fvar(
    fvar: Bytes<'_>,
    pin: Pin,
    out: &mut Out<'_>,
    at: Offset,
) -> Result<Len, Refused> {
    let bad = Refused::Malformed(Tag::FVAR);
    let h = fvar_head(fvar)?;
    if h.axes != pin.axes || h.axis_size != Len::of(20) {
        return Err(bad);
    }
    let ps = has_ps_name(&h)?;
    let mut kept = 0u16;
    let mut i = Index::ZERO;
    while h.instances.holds(i) {
        if at_pin(fvar, &h, i, pin)? {
            kept += 1;
        }
        i = i.next();
    }
    let axes = u16::try_from(h.axes.value() - 1).map_err(|_| bad)?;
    let size = u16::try_from(h.instance_size.value() - 4).map_err(|_| bad)?;
    let mut w = Cursor { out, at };
    for v in [1u16, 0, 16, 2, axes, 20, kept, size] {
        w.put(v)?;
    }
    let mut a = Index::ZERO;
    while h.axes.holds(a) {
        if a != pin.axis {
            let rec = h
                .axes_at
                .plus(Offset::ZERO.at(a, Len::of(20)).ok_or(bad)?.since_start())
                .ok_or(bad)?;
            w.copy(fvar.slice(rec, Len::of(20)).ok_or(bad)?.as_slice())?;
        }
        a = a.next();
    }
    let mut i = Index::ZERO;
    while h.instances.holds(i) {
        if at_pin(fvar, &h, i, pin)? {
            let rec = instance_at(&h, i).ok_or(bad)?;
            w.copy(fvar.slice(rec, Len::of(4)).ok_or(bad)?.as_slice())?;
            let mut a = Index::ZERO;
            while h.axes.holds(a) {
                if a != pin.axis {
                    let c = rec
                        .plus(Len::of(4))
                        .and_then(|o| o.plus(Offset::ZERO.at(a, Len::of(4))?.since_start()))
                        .ok_or(bad)?;
                    w.copy(fvar.slice(c, Len::of(4)).ok_or(bad)?.as_slice())?;
                }
                a = a.next();
            }
            if ps {
                let c = rec.plus(Len::of(4 + 4 * h.axes.value())).ok_or(bad)?;
                w.copy(fvar.slice(c, Len::of(2)).ok_or(bad)?.as_slice())?;
            }
        }
        i = i.next();
    }
    w.at.since(at).ok_or(Refused::Metrics)
}

/// fontTools' `_isValidAvarSegmentMap`: an empty map, or one holding
/// -1, 0 and 1 to themselves whose values never fall.
fn valid_map(records: Bytes<'_>, n: Count) -> Option<bool> {
    if n.is_zero() {
        return Some(true);
    }
    let one = 16384i16;
    let mut need = [(-one, -one), (0, 0), (one, one)].map(|p| (p, false));
    // fontTools sorts by the from value; a map whose from values fall is
    // read in that sorted order, which this walk does not reproduce, so it
    // is refused.
    let mut prev_from: Option<i16> = None;
    let mut prev_to: Option<i16> = None;
    let mut i = Index::ZERO;
    while n.holds(i) {
        let at = Offset::ZERO.at(i, Len::of(4))?;
        let from: F2Dot14 = records.read(at)?;
        let to: F2Dot14 = records.read(at.plus(Len::of(2))?)?;
        if prev_from.is_some_and(|p| from.0 <= p) {
            return None;
        }
        if prev_to.is_some_and(|p| p > to.0) {
            return Some(false);
        }
        for (p, seen) in need.iter_mut() {
            if *p == (from.0, to.0) {
                *seen = true;
            }
        }
        prev_from = Some(from.0);
        prev_to = Some(to.0);
        i = i.next();
    }
    Some(need.iter().all(|(_, s)| *s))
}

/// The new `avar`: version 1.0, the pinned axis's map gone.
pub(super) fn avar(
    avar: Bytes<'_>,
    pin: Pin,
    out: &mut Out<'_>,
    at: Offset,
) -> Result<Len, Refused> {
    let bad = Refused::Malformed(Tag::AVAR);
    let parsed = Avar::parse(avar).ok_or(bad)?;
    let count: u16 = avar.read(Offset::of(6u32)).ok_or(bad)?;
    if Count::of(count) != pin.axes {
        return Err(bad);
    }
    let mut w = Cursor { out, at };
    for v in [1u16, 0, 0, count - 1] {
        w.put(v)?;
    }
    let mut a = Index::ZERO;
    while pin.axes.holds(a) {
        if a != pin.axis {
            let (records, n) = parsed.map(a).ok_or(bad)?;
            if !valid_map(records, n).ok_or(Refused::Table(Tag::AVAR))? {
                return Err(Refused::Table(Tag::AVAR));
            }
            w.put(u16::try_from(n.value()).map_err(|_| bad)?)?;
            w.copy(records.as_slice())?;
        }
        a = a.next();
    }
    w.at.since(at).ok_or(Refused::Metrics)
}

/// `STAT`'s header fields.
struct StatHead {
    header: Len,
    axis_size: Len,
    axes: Count,
    axes_at: Offset,
    values: Count,
    values_at: Offset,
}

fn stat_head(stat: Bytes<'_>) -> Result<StatHead, Refused> {
    let bad = Refused::Malformed(STAT);
    let major: u16 = stat.read(Offset::ZERO).ok_or(bad)?;
    let minor: u16 = stat.read(Offset::of(2u32)).ok_or(bad)?;
    if major != 1 {
        return Err(bad);
    }
    let r16 = |k: u32| stat.read::<u16>(Offset::of(k)).ok_or(bad);
    let r32 = |k: u32| stat.read::<u32>(Offset::of(k)).ok_or(bad);
    Ok(StatHead {
        header: Len::of(if minor >= 1 { 20 } else { 18 }),
        axis_size: Len::of(u32::from(r16(4)?)),
        axes: Count::of(r16(6)?),
        axes_at: Offset::of(r32(8)?),
        values: Count::of(r16(12)?),
        values_at: Offset::of(r32(14)?),
    })
}

/// Axis value `i`'s table: where it is, and how long.
fn stat_value<'s>(stat: Bytes<'s>, h: &StatHead, i: Index) -> Result<Bytes<'s>, Refused> {
    let bad = Refused::Malformed(STAT);
    let off: u16 = stat
        .read(
            h.values_at
                .plus(Offset::ZERO.at(i, Len::of(2)).ok_or(bad)?.since_start())
                .ok_or(bad)?,
        )
        .ok_or(bad)?;
    let t = stat
        .tail(h.values_at.plus(Len::of(u32::from(off))).ok_or(bad)?)
        .ok_or(bad)?;
    let format: u16 = t.read(Offset::ZERO).ok_or(bad)?;
    let len = match format {
        1 => 12,
        2 => 20,
        3 => 16,
        4 => {
            let n: u16 = t.read(Offset::of(2u32)).ok_or(bad)?;
            8 + 6 * u32::from(n)
        }
        _ => return Err(bad),
    };
    t.slice(Offset::ZERO, Len::of(len)).ok_or(bad)
}

/// Design axis `a`'s tag.
fn stat_axis_tag(stat: Bytes<'_>, h: &StatHead, a: Index) -> Option<Tag> {
    let rec = h
        .axes_at
        .plus(Offset::ZERO.at(a, h.axis_size)?.since_start())?;
    Some(Tag::from_bytes(
        stat.slice(rec, Len::of(4))?.as_slice().try_into().ok()?,
    ))
}

/// Whether axis value `t` survives the pin: one on the pinned axis only at
/// the pinned value (`axisValuesFromAxisLimits`).
fn value_kept(stat: Bytes<'_>, h: &StatHead, t: Bytes<'_>, pin: Pin) -> Result<bool, Refused> {
    let bad = Refused::Malformed(STAT);
    let outside = |axis: u16, v: Fixed| -> Result<bool, Refused> {
        let tag = stat_axis_tag(stat, h, Index::of(axis)).ok_or(bad)?;
        Ok(tag == WGHT && v != pin.user)
    };
    let format: u16 = t.read(Offset::ZERO).ok_or(bad)?;
    match format {
        1..=3 => {
            let axis: u16 = t.read(Offset::of(2u32)).ok_or(bad)?;
            // Value at 8 in formats 1 and 3; format 2's nominal value too.
            let v: Fixed = t.read(Offset::of(8u32)).ok_or(bad)?;
            Ok(!outside(axis, v)?)
        }
        4 => {
            let n: u16 = t.read(Offset::of(2u32)).ok_or(bad)?;
            for k in 0..u32::from(n) {
                let axis: u16 = t.read(Offset::of(8 + 6 * k)).ok_or(bad)?;
                let v: Fixed = t.read(Offset::of(10 + 6 * k)).ok_or(bad)?;
                if outside(axis, v)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Err(bad),
    }
}

/// The new `STAT`: the design axes kept, the axis values the pin excludes
/// gone.
pub(super) fn stat(
    stat: Bytes<'_>,
    pin: Pin,
    out: &mut Out<'_>,
    at: Offset,
) -> Result<Len, Refused> {
    let bad = Refused::Malformed(STAT);
    let h = stat_head(stat)?;
    let mut kept = 0u16;
    let mut i = Index::ZERO;
    while h.values.holds(i) {
        if value_kept(stat, &h, stat_value(stat, &h, i)?, pin)? {
            kept += 1;
        }
        i = i.next();
    }
    let axes_len = h.axis_size.times(h.axes).ok_or(bad)?;
    let axes_at = h.header;
    let values_at = Len::of(axes_at.value() + axes_len.value());
    let mut w = Cursor { out, at };
    w.copy(stat.slice(Offset::ZERO, Len::of(8)).ok_or(bad)?.as_slice())?;
    w.put(if h.axes.is_zero() { 0 } else { axes_at.value() })?;
    w.put(kept)?;
    w.put(if kept == 0 { 0 } else { values_at.value() })?;
    if h.header == Len::of(20) {
        w.copy(
            stat.slice(Offset::of(18u32), Len::of(2))
                .ok_or(bad)?
                .as_slice(),
        )?;
    }
    w.copy(stat.slice(h.axes_at, axes_len).ok_or(bad)?.as_slice())?;
    let offsets = w.at;
    let mut body = 2 * u32::from(kept);
    for _ in 0..kept {
        w.put(0u16)?;
    }
    let mut k = 0u32;
    let mut i = Index::ZERO;
    while h.values.holds(i) {
        let t = stat_value(stat, &h, i)?;
        if value_kept(stat, &h, t, pin)? {
            w.out.put(
                offsets.plus(Len::of(2 * k)).ok_or(Refused::Metrics)?,
                u16::try_from(body).map_err(|_| bad)?,
            );
            w.copy(t.as_slice())?;
            body += t.len().value();
            k += 1;
        }
        i = i.next();
    }
    w.at.since(at).ok_or(Refused::Metrics)
}

/// The name IDs past 255 that `fvar` and `STAT` name (`getVariationNameIDs`),
/// handed one by one to `f`; `pinned` reads them after the pin.
fn variation_names(
    fvar: Bytes<'_>,
    stat: Option<Bytes<'_>>,
    pin: Option<Pin>,
    f: &mut impl FnMut(u16),
) -> Result<(), Refused> {
    let bad = Refused::Malformed(Tag::FVAR);
    let h = fvar_head(fvar)?;
    let mut put = |id: u16| {
        if id > 255 {
            f(id);
        }
    };
    let mut a = Index::ZERO;
    while h.axes.holds(a) {
        if pin.is_none_or(|p| p.axis != a) {
            let rec = h
                .axes_at
                .plus(Offset::ZERO.at(a, h.axis_size).ok_or(bad)?.since_start())
                .ok_or(bad)?;
            put(fvar.read(rec.plus(Len::of(18)).ok_or(bad)?).ok_or(bad)?);
        }
        a = a.next();
    }
    let ps = has_ps_name(&h)?;
    let mut i = Index::ZERO;
    while h.instances.holds(i) {
        let keep = match pin {
            Some(p) => at_pin(fvar, &h, i, p)?,
            None => true,
        };
        if keep {
            let rec = instance_at(&h, i).ok_or(bad)?;
            put(fvar.read(rec).ok_or(bad)?);
            if ps {
                let id: u16 = fvar
                    .read(rec.plus(Len::of(4 + 4 * h.axes.value())).ok_or(bad)?)
                    .ok_or(bad)?;
                if id != 0xFFFF {
                    put(id);
                }
            }
        }
        i = i.next();
    }
    if let Some(stat) = stat {
        let sbad = Refused::Malformed(STAT);
        let sh = stat_head(stat)?;
        let mut a = Index::ZERO;
        while sh.axes.holds(a) {
            let rec = sh
                .axes_at
                .plus(Offset::ZERO.at(a, sh.axis_size).ok_or(sbad)?.since_start())
                .ok_or(sbad)?;
            put(stat.read(rec.plus(Len::of(4)).ok_or(sbad)?).ok_or(sbad)?);
            a = a.next();
        }
        let mut i = Index::ZERO;
        while sh.values.holds(i) {
            let t = stat_value(stat, &sh, i)?;
            if pin.is_none() || value_kept(stat, &sh, t, pin.ok_or(sbad)?)? {
                put(t.read(Offset::of(6u32)).ok_or(sbad)?);
            }
            i = i.next();
        }
        if sh.header == Len::of(20) {
            put(stat.read(Offset::of(18u32)).ok_or(sbad)?);
        }
    }
    Ok(())
}

/// A `name` record's nameID.
#[derive(Clone, Copy)]
struct NameId(u16);

/// Whether `id` is one the pin leaves unnamed: named before it, not after.
fn pruned(fvar: Bytes<'_>, stat: Option<Bytes<'_>>, pin: Pin, id: NameId) -> Result<bool, Refused> {
    let NameId(id) = id;
    let (mut before, mut after) = (false, false);
    variation_names(fvar, stat, None, &mut |n| before |= n == id)?;
    variation_names(fvar, stat, Some(pin), &mut |n| after |= n == id)?;
    Ok(before && !after)
}

/// The new `name`: the records only the dropped instances and axis values
/// named are gone; the string storage is kept whole.
pub(super) fn name(
    name: Bytes<'_>,
    fvar: Bytes<'_>,
    stat: Option<Bytes<'_>>,
    pin: Pin,
    out: &mut Out<'_>,
    at: Offset,
) -> Result<Len, Refused> {
    let bad = Refused::Malformed(Tag::NAME);
    let format: u16 = name.read(Offset::ZERO).ok_or(bad)?;
    if format != 0 {
        return Err(Refused::Table(Tag::NAME));
    }
    let count: u16 = name.read(Offset::of(2u32)).ok_or(bad)?;
    let storage: u16 = name.read(Offset::of(4u32)).ok_or(bad)?;
    let strings = name.tail(Offset::of(storage)).ok_or(bad)?;
    let record = |k: u32| name.slice(Offset::of(6 + 12 * k), Len::of(12)).ok_or(bad);
    let mut kept = 0u32;
    for k in 0..u32::from(count) {
        let id: u16 = record(k)?.read(Offset::of(6u32)).ok_or(bad)?;
        if !pruned(fvar, stat, pin, NameId(id))? {
            kept += 1;
        }
    }
    let mut w = Cursor { out, at };
    w.put(0u16)?;
    w.put(u16::try_from(kept).map_err(|_| bad)?)?;
    w.put(u16::try_from(6 + 12 * kept).map_err(|_| bad)?)?;
    for k in 0..u32::from(count) {
        let r = record(k)?;
        let id: u16 = r.read(Offset::of(6u32)).ok_or(bad)?;
        if !pruned(fvar, stat, pin, NameId(id))? {
            w.copy(r.as_slice())?;
        }
    }
    w.copy(strings.as_slice())?;
    w.at.since(at).ok_or(Refused::Metrics)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec::Vec;

    /// `iup_contour_optimize(deltas, coords, 0.5)` on each case, with the
    /// points it kept: fontTools' own output, most of them from the million
    /// random contours the port was held to. The first two are the ones
    /// that caught it: a search range one short, which only the last start
    /// of the doubled list reaches, and a tolerance test at the edge of 0.5.
    const CASES: [(&str, &[usize]); 8] = [
        (
            "4 13 -1 1;12 13 0 1;3 11 -1 1;17 -12 0 0;5 -4 -1 0",
            &[1, 4],
        ),
        (
            "8 16 -1 1;8 -15 0 -1;11 -3 -1 0;13 -16 -2 -1;11 20 -1 1;9 4 -1 0;5 -6 0 0;-9 -20 1 -1;-3 1 1 0;17 13 -2 1;3 5 0 0;-6 5 1 0;3 19 1 1;11 -13 -1 -1;-5 -8 2 -1;4 -13 0 -1;10 4 -1 0;16 9 -2 1;-3 -9 0 -1;-1 -16 0 -1;-8 20 1 1;-11 -13 1 -1;-14 -4 2 0;4 11 1 1",
            &[0, 1, 2, 3, 4, 7, 8, 9, 11, 12, 13, 14, 17, 21, 22, 23],
        ),
        (
            "64 174 3 -12;-24 -93 0 6;200 -97 9 7;109 -10 6 1;173 132 8 -9;31 60 2 -4;-180 63 -8 -4;-133 163 -6 -11",
            &[0, 1, 2, 3, 4, 6],
        ),
        (
            "505 -670 2 1;-683 -542 -2 3;431 826 2 -1;-432 -608 3 0;-264 514 -2 -2;544 -221 -3 -3",
            &[0, 1, 2, 3, 4, 5],
        ),
        (
            "-163 50 0 1;-38 101 0 0;16 -37 261 1;55 -135 0 0;-30 24 -1 1",
            &[1, 2, 3, 4],
        ),
        (
            "-146 123 19 -4;-138 -46 18 1;-42 -23 5 1;17 80 -1 -2;-25 -148 3 5;66 152 -8 -5;174 196 -22 -6;-145 -86 19 3;-195 139 25 -4;28 121 -4 -4",
            &[0, 2, 3, 4, 5, 6, 7, 8, 9],
        ),
        ("-15 -20 0 0;10 8 0 0;-20 11 0 0;8 17 0 0;10 -17 0 0", &[]),
        ("5 5 3 4;7 7 3 4;9 1 3 4", &[0]),
    ];

    fn points(case: &str) -> Vec<BakePoint> {
        case.split(';')
            .map(|p| {
                let v: Vec<f64> = p.split(' ').map(|x| x.parse().unwrap()).collect();
                let mut q = BakePoint::at(v[0], v[1], LastPoint::No);
                q.ax = v[2];
                q.ay = v[3];
                q
            })
            .collect()
    }

    #[test]
    fn the_optimiser_keeps_the_points_fonttools_keeps() {
        let mut steps = std::vec![IupStep::ZERO; 1 << 8];
        for (case, keep) in CASES {
            let mut c = points(case);
            optimize_contour(&mut c, &mut steps).unwrap();
            let got: Vec<usize> = c
                .iter()
                .enumerate()
                .filter(|(_, p)| p.choice == Choice::Explicit)
                .map(|(i, _)| i)
                .collect();
            assert_eq!(got, keep, "{case}");
        }
    }

    #[test]
    fn a_contour_past_the_steps_is_refused() {
        let mut steps = std::vec![IupStep::ZERO; 4];
        let mut c = points(CASES[0].0);
        assert_eq!(optimize_contour(&mut c, &mut steps), Err(Refused::Scratch));
    }

    /// The edge of the tolerance, where this Mac's `hypot` says 0.5.
    #[test]
    fn the_tolerance_is_inclusive_and_read_in_doubles() {
        assert!(within_half(0.5, 0.0));
        assert!(within_half(0.40000000000000013, 0.29999999999999993));
        assert!(!within_half(0.5000000000000001, 0.0));
        assert!(!within_half(0.4, 0.31));
    }

    fn deltas(values: &[i16]) -> Vec<u8> {
        let mut b = std::vec![0u8; 256];
        let mut out = Out { buf: &mut b };
        let mut w = Cursor {
            out: &mut out,
            at: Offset::ZERO,
        };
        put_deltas(values, &mut w).unwrap();
        let n = usize::try_from(w.at.since_start().value()).unwrap();
        b[..n].to_vec()
    }

    /// `compileDeltaValues_`'s own examples, from its comments.
    #[test]
    fn the_deltas_pack_as_fonttools_comments_say() {
        assert_eq!(deltas(&[15, 15, 0, 15, 15]), [4, 15, 15, 0, 15, 15]);
        assert_eq!(
            deltas(&[0x6666, 0, 0x7777]),
            [0x40, 0x66, 0x66, 0x80, 0x40, 0x77, 0x77]
        );
        assert_eq!(
            deltas(&[0x6666, 2, 0x7777]),
            [0x42, 0x66, 0x66, 0, 2, 0x77, 0x77]
        );
        assert_eq!(deltas(&[0; 65]), [0x80 | 63, 0x80]);
    }

    /// `compilePoints`' own example: the set {17..=23} is its count, the
    /// run's length less one, the first number and the differences.
    #[test]
    fn the_points_pack_as_fonttools_comments_say() {
        let mut pts = std::vec![BakePoint::ZERO; 24];
        for (i, p) in pts.iter_mut().enumerate() {
            p.choice = if (17..=23).contains(&i) {
                Choice::Explicit
            } else {
                Choice::Implied
            };
        }
        let mut b = std::vec![0u8; 16];
        let mut out = Out { buf: &mut b };
        let mut w = Cursor {
            out: &mut out,
            at: Offset::ZERO,
        };
        put_points(&pts, &mut w).unwrap();
        assert_eq!(b[..9], [7, 6, 17, 1, 1, 1, 1, 1, 1]);
    }
}
