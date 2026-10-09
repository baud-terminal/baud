//! **WCAG 2.3.1's flashes, counted on frames**: the general flash and the red
//! flash, per pixel, per one-second interval, and the area flashing more than
//! three times inside any 341 x 256-point window.
//!
//! The texts (`docs/research/sources/`, key `wcag22`, and its Understanding
//! page, `wcag22-understanding-three-flashes`), as this module reads them:
//!
//! * **A general flash** is "a pair of opposing changes in relative luminance
//!   of 10% or more of the maximum relative luminance (1.0) where the relative
//!   luminance of the darker image is below 0.80; and where 'a pair of opposing
//!   changes' is an increase followed by a decrease, or a decrease followed by
//!   an increase". Relative luminance is WCAG 2.2's: the piecewise sRGB curve
//!   (0.04045, exponent 2.4) and `0.2126 R + 0.7152 G + 0.0722 B`.
//! * **A red flash** is "any pair of opposing transitions involving a
//!   saturated red", and the Understanding page's working definition: "one
//!   transition is either to or from a state with a value R/(R + G + B) that
//!   is greater than or equal to 0.8, and the difference between states is
//!   more than 0.2 (unitless) in the CIE 1976 UCS chromaticity diagram", u'v'
//!   from XYZ by its own matrix, "where R, G, & B are values that range from
//!   0-1 as specified in 'relative luminance' definition".
//! * **The threshold** passes content if "there are no more than three general
//!   flashes and / or no more than three red flashes within any one-second
//!   period; or the combined area of flashes occurring concurrently occupies
//!   no more than ... 25% of any 10 degree visual field", the field estimated
//!   by Note 1's "341 x 256 pixel rectangle anywhere on the displayed screen
//!   area when the content is viewed at 1024 x 768 pixels".
//!
//! **This is this project's implementation of those definitions**, not a
//! port of any analyser's (PEAT's or Harding's practice was not read). Every
//! choice the texts leave open is made here once and named:
//!
//! * **A transition is measured from the last extremum, not frame to frame.**
//!   Each pixel's luminance is followed as a zig-zag with a 0.10 hysteresis:
//!   a turn is confirmed when the value comes back 0.10 from the running
//!   extremum, and each leg between turns is one change, ending where its
//!   extremum was first reached. A leg counts toward a general flash when its
//!   darker end is below 0.80. A fade spread over many frames is one change,
//!   which a frame-to-frame test would never see.
//! * **A red transition** is a change of state, from the state the last red
//!   transition left (or the first frame), to one more than 0.2 away in u'v'
//!   where either state is a saturated red. Toward red when the new state is
//!   the red one, away when the old one is. R/(R+G+B) is taken on the linear
//!   values, the "relative luminance" definition's R, G and B; the encoded
//!   ratio, which note AJ's model used, is counted as a second series. Black
//!   has no chromaticity (X + 15Y + 3Z is zero): it is given the white
//!   point's, the u'v' of R = G = B, and is not a saturated red.
//! * **Transitions pair into flashes disjointly** (a transition belongs to at
//!   most one flash, paired in time order with the next one of the opposite
//!   direction), so a flash at f Hz is 2f transitions a second and "more than
//!   three" needs eight in the second. **The chained reading** -- every
//!   adjacent opposing pair a flash, so n transitions are n - 1 flashes and
//!   five are "more than three" -- is counted beside it, as the strict
//!   verdict.
//! * **A one-second period is any 1 s of frames**, `[s, s + rate)`, sliding a
//!   frame at a time, and a transition is in it when the frame its change
//!   completed on is.
//! * **"Concurrently" is "within the same one-second period"**: the area is
//!   every pixel that flashes more than three times inside one period,
//!   general or red, summed inside the window whether or not it is
//!   contiguous. Both make the area an upper bound on the Understanding
//!   page's "contiguous area that is flashing simultaneously"; the worst
//!   window's largest 8-connected piece is reported beside it, so a verdict
//!   that rests on summing pieces apart can be seen to.
//! * **Pixels can be left out of a second area, never out of the frames.**
//!   [`Census::exclude`] names rectangles whose flashes still count in the
//!   verdict's area and not in [`Report::apart`]; the frames are counted as
//!   they were drawn either way.
//! * **The window slides a pixel at a time** over every position inside the
//!   frame, `round(341 s) x round(256 s)` device pixels at `s` pixels a
//!   point. Areas are reported in points squared, so the threshold is 21,824
//!   at every scale.

use baud_font::Px;
use baud_render::Tally;

/// A general flash's change: 10% of the maximum relative luminance.
pub const CHANGE: f32 = 0.10;

/// The darker image of a general flash is below this.
pub const DARKER: f32 = 0.80;

/// A saturated red has R/(R + G + B) at least this.
pub const RED: f32 = 0.8;

/// A red transition's chromaticity difference is more than this.
pub const STEP: f32 = 0.2;

/// Note 1's 10-degree field, in points (pixels at 1024 x 768).
pub const FIELD: (f64, f64) = (341.0, 256.0);

/// The share of the field the concurrent flashing may occupy.
pub const SHARE: f64 = 0.25;

/// "No more than three" flashes in a second.
pub const MOST: Flashes = Flashes(3);

/// A frame's place in a run, from 0.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Frame(pub u32);

/// Frames a second.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rate(pub u32);

/// Device pixels a point: 1 or 2 on a Mac.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scale(pub u32);

/// Flashes inside one second.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Flashes(pub u32);

/// Transitions inside one second.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Transitions(pub u32);

/// An area in points squared.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug, Default)]
pub struct Area(pub f64);

/// Whether a run passes 2.3.1's threshold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Passes,
    Fails,
}

/// How transitions pair into flashes: see the module doc.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pairing {
    /// A transition in at most one flash.
    Disjoint,
    /// Every adjacent opposing pair is a flash.
    Chained,
}

/// One series' worst second: its most transitions and flashes at any pixel
/// in any one-second period; the series' own concurrent area of more than
/// three, disjoint; and the area of the pixels that flash at least once in
/// some period, in the worst window (a union over the run, not a concurrent
/// area).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Series {
    pub transitions: Transitions,
    pub flashes: Flashes,
    pub chained: Flashes,
    pub over: Area,
    pub ever: Area,
}

/// **A run's count.**
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Report {
    /// General flashes.
    pub general: Series,
    /// Red flashes, R/(R+G+B) on the linear values.
    pub red: Series,
    /// Red flashes, R/(R+G+B) on the encoded values.
    pub red_encoded: Series,
    /// The verdict's area: the most of any window that flashes more than
    /// three times, general or red (linear), inside one second, disjoint
    /// pairing.
    pub area: Area,
    /// Of that area, in that window and that second, the largest 8-connected
    /// piece: what the verdict's area is without summing pieces apart.
    pub contiguous: Area,
    /// When that second starts, seconds into the run.
    pub second: f64,
    /// The strict area: general, red and red-encoded, chained pairing.
    pub strict: Area,
    /// The verdict's area with the pixels [`Census::exclude`] named left
    /// out: what flashes apart from them. The frames are not touched; their
    /// pixels are only not counted, so this is a lower bound on what else
    /// flashes there.
    pub apart: Area,
    /// The strict area, the same pixels left out.
    pub apart_strict: Area,
    /// 25% of the field: 21,824.
    pub threshold: Area,
    /// Frames counted.
    pub frames: Frame,
    /// Pixel-frames whose alpha was not opaque: a luminance read off them is
    /// not what a screen shows, so a run with any is not a measurement.
    pub translucent: Tally,
}

impl Report {
    /// The verdict on the disjoint reading.
    pub fn outcome(&self) -> Outcome {
        if self.area > self.threshold {
            Outcome::Fails
        } else {
            Outcome::Passes
        }
    }

    /// The verdict on the chained reading, with both red ratios.
    pub fn strict_outcome(&self) -> Outcome {
        if self.strict > self.threshold {
            Outcome::Fails
        } else {
            Outcome::Passes
        }
    }

    /// The verdict with the excluded pixels left out, disjoint.
    pub fn apart_outcome(&self) -> Outcome {
        if self.apart > self.threshold {
            Outcome::Fails
        } else {
            Outcome::Passes
        }
    }

    /// The same, chained.
    pub fn apart_strict_outcome(&self) -> Outcome {
        if self.apart_strict > self.threshold {
            Outcome::Fails
        } else {
            Outcome::Passes
        }
    }
}

/// A pixel's place in the frame, row-major.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Pixel(u32);

/// Which way a change went: a luminance increase or decrease, or a red
/// transition toward the red state or away from it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    Up,
    Down,
}

impl Dir {
    fn opposite(self) -> Dir {
        match self {
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
        }
    }
}

/// One transition: where, on which frame it completed, which way.
#[derive(Clone, Copy, Debug)]
struct Event {
    pixel: Pixel,
    at: Frame,
    dir: Dir,
}

/// Where a pixel's luminance zig-zag stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Leg {
    /// No change of 0.10 yet: the anchor is the lowest value so far and the
    /// extremum the highest.
    Unset,
    Rising,
    Falling,
}

/// **A pixel's general-flash state**: the leg in progress, from its anchor
/// (the last turn, valued `a` on frame `at`) to its running extremum (`e`,
/// first reached on frame `et`).
#[derive(Clone, Copy, Debug)]
struct Zig {
    leg: Leg,
    a: f32,
    at: Frame,
    e: f32,
    et: Frame,
}

/// Whether a state is a saturated red.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Redness {
    Red,
    Not,
}

/// **A pixel's red-flash state**: the state the last red transition left,
/// as u'v' and whether it is a saturated red, and that transition's way.
#[derive(Clone, Copy, Debug)]
struct Hue {
    u: f32,
    v: f32,
    red: Redness,
    last: Option<Dir>,
}

/// A pixel's colour as the definitions read it.
#[derive(Clone, Copy, Debug)]
struct Seen {
    lum: f32,
    u: f32,
    v: f32,
    red: Redness,
    red_encoded: Redness,
}

/// A run of one-second periods, by their first frame (`from` inclusive, `to`
/// exclusive, either may lie before frame 0), in which a pixel flashes more
/// than three times.
#[derive(Clone, Copy, Debug)]
struct Span {
    from: Start,
    to: Start,
    pixel: Pixel,
}

/// A one-second period's first frame, which may be before the run's.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Start(i64);

/// A box of device pixels, `right` and `bottom` inclusive.
#[derive(Clone, Copy, Debug)]
struct Bounds {
    left: Px,
    top: Px,
    right: Px,
    bottom: Px,
}

/// The densest window of a set: its count as points squared, and its
/// top-left device pixel.
#[derive(Clone, Copy, Debug)]
struct Densest {
    area: Area,
    left: Px,
    top: Px,
}

/// The worst one-second period of a set of spans: the densest window's area,
/// its largest connected piece, and when the period starts, in seconds.
#[derive(Clone, Copy, Debug)]
struct Worst {
    area: Area,
    contiguous: Area,
    second: f64,
}

/// One series' per-pixel results, before areas.
#[derive(Default)]
struct Tallied {
    transitions: Transitions,
    flashes: Flashes,
    chained: Flashes,
    /// Pixels that flash at least once in some second.
    ever: Vec<Pixel>,
    /// Periods in which a pixel flashes more than three times, disjointly.
    over: Vec<Span>,
    /// The same, chained.
    over_chained: Vec<Span>,
}

/// u'v' of R = G = B, by the Understanding page's matrix: what black is given.
fn white_point() -> (f32, f32) {
    let x = 0.412_456_4 + 0.357_576_1 + 0.180_437_5;
    let y = 0.212_672_9 + 0.715_152_2 + 0.072_175;
    let z = 0.019_333_9 + 0.119_192 + 0.950_304_1;
    let d = x + 15.0 * y + 3.0 * z;
    (4.0 * x / d, 9.0 * y / d)
}

/// The flashes `events` (one pixel's, in time order) make.
fn pairs(events: &[Event], how: Pairing) -> Flashes {
    let mut n = 0u32;
    match how {
        Pairing::Disjoint => {
            let mut open: Option<Dir> = None;
            for e in events {
                if open == Some(e.dir.opposite()) {
                    n += 1;
                    open = None;
                } else {
                    open = Some(e.dir);
                }
            }
        }
        Pairing::Chained => {
            for w in events.windows(2) {
                n += u32::from(w[0].dir != w[1].dir);
            }
        }
    }
    Flashes(n)
}

/// **The periods in which one pixel flashes more than three times**, as
/// spans of their first frames. The set of transitions inside `[s, s +
/// rate)` changes only where a transition enters (`s = t - rate + 1`) or
/// leaves (`s = t + 1`), so the count is taken once between each two such
/// points.
fn spans(events: &[Event], rate: Rate, how: Pairing, out: &mut Vec<Span>) {
    let r = i64::from(rate.0);
    let mut cuts: Vec<i64> = events
        .iter()
        .flat_map(|e| [i64::from(e.at.0) - r + 1, i64::from(e.at.0) + 1])
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let (mut lo, mut hi) = (0usize, 0usize);
    for k in 0..cuts.len().saturating_sub(1) {
        let s = cuts[k];
        while lo < events.len() && i64::from(events[lo].at.0) < s {
            lo += 1;
        }
        while hi < events.len() && i64::from(events[hi].at.0) < s + r {
            hi += 1;
        }
        if pairs(&events[lo..hi.max(lo)], how) > MOST {
            let pixel = events[0].pixel;
            match out.last_mut() {
                Some(last) if last.pixel == pixel && last.to == Start(s) => {
                    last.to = Start(cuts[k + 1]);
                }
                _ => out.push(Span {
                    from: Start(s),
                    to: Start(cuts[k + 1]),
                    pixel,
                }),
            }
        }
    }
}

/// **One series' transitions, pixel by pixel**: the most in any second, the
/// pixels that ever flash, and the periods each flashes more than three
/// times in.
fn tally(mut events: Vec<Event>, rate: Rate) -> Tallied {
    events.sort_unstable_by_key(|e| (e.pixel, e.at));
    let mut t = Tallied::default();
    for group in events.chunk_by(|a, b| a.pixel == b.pixel) {
        let (mut most, mut chained) = (Flashes(0), Flashes(0));
        let mut end = 0usize;
        for i in 0..group.len() {
            let close = u64::from(group[i].at.0) + u64::from(rate.0);
            while end < group.len() && u64::from(group[end].at.0) < close {
                end += 1;
            }
            let second = &group[i..end];
            let n = Transitions(u32::try_from(second.len()).unwrap_or(u32::MAX));
            t.transitions = t.transitions.max(n);
            most = most.max(pairs(second, Pairing::Disjoint));
            chained = chained.max(pairs(second, Pairing::Chained));
        }
        t.flashes = t.flashes.max(most);
        t.chained = t.chained.max(chained);
        if most > Flashes(0) {
            t.ever.push(group[0].pixel);
        }
        if most > MOST {
            spans(group, rate, Pairing::Disjoint, &mut t.over);
        }
        if chained > MOST {
            spans(group, rate, Pairing::Chained, &mut t.over_chained);
        }
    }
    t
}

/// **The counter for one run**: fed every frame in order, then finished.
pub struct Census {
    w: Px,
    h: Px,
    scale: Scale,
    rate: Rate,
    frame: Frame,
    translucent: Tally,
    /// WCAG's curve, a byte at a time.
    lin: [f32; 256],
    white: (f32, f32),
    prev: Vec<u32>,
    zig: Vec<Zig>,
    hue: Vec<Hue>,
    hue_encoded: Vec<Hue>,
    general: Vec<Event>,
    red: Vec<Event>,
    red_encoded: Vec<Event>,
    /// Rectangles left out of the apart areas.
    apart: Vec<Bounds>,
}

/// Whether a pixel counts toward the apart areas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Exclusion {
    In,
    Out,
}

impl Census {
    /// A census of `width` x `height` device pixels at `scale` pixels a
    /// point, `rate` frames a second.
    pub fn new(width: Px, height: Px, scale: Scale, rate: Rate) -> Census {
        let mut lin = [0.0f32; 256];
        for (b, slot) in (0u8..=255).zip(lin.iter_mut()) {
            let c = f64::from(b) / 255.0;
            *slot = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            } as f32;
        }
        Census {
            w: width,
            h: height,
            scale,
            rate,
            frame: Frame(0),
            translucent: Tally::default(),
            lin,
            white: white_point(),
            prev: Vec::new(),
            zig: Vec::new(),
            hue: Vec::new(),
            hue_encoded: Vec::new(),
            general: Vec::new(),
            red: Vec::new(),
            red_encoded: Vec::new(),
            apart: Vec::new(),
        }
    }

    /// **Leave a rectangle of device pixels out of [`Report::apart`]**, `left`
    /// and `top` its first pixel: they are still counted in the verdict's
    /// area, which is the screen's.
    pub fn exclude(&mut self, left: Px, top: Px, width: Px, height: Px) {
        let last = |a: Px, n: Px| Px::of(a.value().saturating_add(n.value().saturating_sub(1)));
        if width.value() > 0 && height.value() > 0 {
            self.apart.push(Bounds {
                left,
                top,
                right: last(left, width),
                bottom: last(top, height),
            });
        }
    }

    /// Whether `p` lies in a rectangle [`Census::exclude`] named.
    fn excluded(&self, p: Pixel) -> Exclusion {
        let (w, _) = self.sides();
        let i = usize::try_from(p.0).unwrap_or(0);
        let x = u32::try_from(i % w.max(1)).unwrap_or(u32::MAX);
        let y = u32::try_from(i / w.max(1)).unwrap_or(u32::MAX);
        let inside = |b: &Bounds| {
            x >= b.left.value()
                && x <= b.right.value()
                && y >= b.top.value()
                && y <= b.bottom.value()
        };
        if self.apart.iter().any(inside) {
            Exclusion::Out
        } else {
            Exclusion::In
        }
    }

    /// `spans` without the excluded pixels'.
    fn kept(&self, spans: &[Span]) -> Vec<Span> {
        spans
            .iter()
            .filter(|s| self.excluded(s.pixel) == Exclusion::In)
            .copied()
            .collect()
    }

    /// The frame's sides in pixels, as indices.
    fn sides(&self) -> (usize, usize) {
        (
            usize::try_from(self.w.value()).unwrap_or(0),
            usize::try_from(self.h.value()).unwrap_or(0),
        )
    }

    /// A word as the definitions read it.
    fn see(&self, p: u32) -> Seen {
        let [_, r, g, b] = p.to_be_bytes();
        let (lr, lg, lb) = (
            self.lin[usize::from(r)],
            self.lin[usize::from(g)],
            self.lin[usize::from(b)],
        );
        let lum = 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
        let x = 0.412_456_4 * lr + 0.357_576_1 * lg + 0.180_437_5 * lb;
        let y = 0.212_672_9 * lr + 0.715_152_2 * lg + 0.072_175 * lb;
        let z = 0.019_333_9 * lr + 0.119_192 * lg + 0.950_304_1 * lb;
        let d = x + 15.0 * y + 3.0 * z;
        let (u, v) = if d > 0.0 {
            (4.0 * x / d, 9.0 * y / d)
        } else {
            self.white
        };
        let ratio = |r: f32, sum: f32| {
            if sum > 0.0 && r / sum >= RED {
                Redness::Red
            } else {
                Redness::Not
            }
        };
        let (er, eg, eb) = (f32::from(r), f32::from(g), f32::from(b));
        Seen {
            lum,
            u,
            v,
            red: ratio(lr, lr + lg + lb),
            red_encoded: ratio(er, er + eg + eb),
        }
    }

    /// The next frame: `0xAARRGGBB` words, row-major, `width` to a row.
    pub fn frame(&mut self, pixels: &[u32]) {
        let (w, h) = self.sides();
        assert_eq!(pixels.len(), w * h, "a frame is width x height words");
        let t = self.frame;
        if t == Frame(0) {
            self.prev = pixels.to_vec();
            self.zig = Vec::with_capacity(pixels.len());
            self.hue = Vec::with_capacity(pixels.len());
            self.hue_encoded = Vec::with_capacity(pixels.len());
            for &p in pixels {
                let s = self.see(p);
                self.zig.push(Zig {
                    leg: Leg::Unset,
                    a: s.lum,
                    at: t,
                    e: s.lum,
                    et: t,
                });
                let hue = |red| Hue {
                    u: s.u,
                    v: s.v,
                    red,
                    last: None,
                };
                self.hue.push(hue(s.red));
                self.hue_encoded.push(hue(s.red_encoded));
            }
        }
        let mut thin = 0usize;
        for (i, &p) in pixels.iter().enumerate() {
            thin += usize::from(p >> 24 != 0xFF);
            if p == self.prev[i] {
                continue;
            }
            self.prev[i] = p;
            let s = self.see(p);
            let pixel = Pixel(u32::try_from(i).unwrap_or(u32::MAX));
            zig(&mut self.zig[i], s.lum, t, pixel, &mut self.general);
            hue(&mut self.hue[i], &s, s.red, t, pixel, &mut self.red);
            hue(
                &mut self.hue_encoded[i],
                &s,
                s.red_encoded,
                t,
                pixel,
                &mut self.red_encoded,
            );
        }
        self.translucent.add(Tally::of(thin));
        self.frame = Frame(t.0 + 1);
    }

    /// The field in device pixels, at most the frame.
    fn field(&self) -> (usize, usize) {
        let (w, h) = self.sides();
        let s = f64::from(self.scale.0);
        let side = |pt: f64, limit: usize| {
            usize::try_from((pt * s).round() as i64)
                .unwrap_or(0)
                .clamp(1, limit.max(1))
        };
        (side(FIELD.0, w), side(FIELD.1, h))
    }

    /// Device pixels squared as points squared.
    fn points(&self, n: u64) -> Area {
        let s = f64::from(self.scale.0);
        Area(n as f64 / (s * s))
    }

    /// **The densest window**: the most of `pixels` inside any field-sized
    /// window wholly in the frame, as an area in points squared.
    fn densest(&self, pixels: &[Pixel]) -> Area {
        self.densest_at(pixels).area
    }

    /// The same, and where that window is.
    fn densest_at(&self, pixels: &[Pixel]) -> Densest {
        let none = Densest {
            area: Area(0.0),
            left: Px::of(0),
            top: Px::of(0),
        };
        let (w, h) = self.sides();
        if pixels.is_empty() || w == 0 || h == 0 {
            return none;
        }
        let (ww, wh) = self.field();
        let at = |p: Pixel| {
            let i = usize::try_from(p.0).unwrap_or(0);
            (i % w, i / w)
        };
        let px = |v: usize| Px::of(u32::try_from(v).unwrap_or(u32::MAX));
        let un = |v: Px| usize::try_from(v.value()).unwrap_or(0);
        let b = pixels.iter().fold(None::<Bounds>, |b, &p| {
            let (x, y) = at(p);
            Some(match b {
                None => Bounds {
                    left: px(x),
                    top: px(y),
                    right: px(x),
                    bottom: px(y),
                },
                Some(b) => Bounds {
                    left: px(un(b.left).min(x)),
                    top: px(un(b.top).min(y)),
                    right: px(un(b.right).max(x)),
                    bottom: px(un(b.bottom).max(y)),
                },
            })
        });
        let Some(b) = b else {
            return none;
        };
        // Window origins that touch the box, kept inside the frame.
        let x0 = (un(b.left) + 1).saturating_sub(ww).min(w - ww);
        let x1 = un(b.right).min(w - ww);
        let y0 = (un(b.top) + 1).saturating_sub(wh).min(h - wh);
        let y1 = un(b.bottom).min(h - wh);
        // A summed-area table over the region those windows cover.
        let (rw, rh) = (x1 + ww - x0, y1 + wh - y0);
        let mut grid = vec![0u8; rw * rh];
        for &p in pixels {
            let (x, y) = at(p);
            if (x0..x0 + rw).contains(&x) && (y0..y0 + rh).contains(&y) {
                grid[(y - y0) * rw + (x - x0)] = 1;
            }
        }
        let mut sat = vec![0u64; (rw + 1) * (rh + 1)];
        for y in 0..rh {
            let mut row = 0u64;
            for x in 0..rw {
                row += u64::from(grid[y * rw + x]);
                sat[(y + 1) * (rw + 1) + x + 1] = sat[y * (rw + 1) + x + 1] + row;
            }
        }
        let (mut best, mut origin) = (0u64, (x0, y0));
        for y in 0..=(y1 - y0) {
            for x in 0..=(x1 - x0) {
                let (xb, yb) = (x + ww, y + wh);
                let n = sat[yb * (rw + 1) + xb] + sat[y * (rw + 1) + x]
                    - sat[y * (rw + 1) + xb]
                    - sat[yb * (rw + 1) + x];
                if n > best {
                    best = n;
                    origin = (x0 + x, y0 + y);
                }
            }
        }
        Densest {
            area: self.points(best),
            left: px(origin.0),
            top: px(origin.1),
        }
    }

    /// **The largest 8-connected piece of `pixels` inside the field-sized
    /// window at `left`, `top`**, as an area in points squared.
    fn piece(&self, pixels: &[Pixel], left: Px, top: Px) -> Area {
        let (w, _) = self.sides();
        let (ww, wh) = self.field();
        let (x0, y0) = (
            usize::try_from(left.value()).unwrap_or(0),
            usize::try_from(top.value()).unwrap_or(0),
        );
        let mut grid = vec![0u8; ww * wh];
        for p in pixels {
            let i = usize::try_from(p.0).unwrap_or(0);
            let (x, y) = (i % w, i / w);
            if (x0..x0 + ww).contains(&x) && (y0..y0 + wh).contains(&y) {
                grid[(y - y0) * ww + (x - x0)] = 1;
            }
        }
        let mut most = 0u64;
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for start in 0..grid.len() {
            if grid[start] != 1 {
                continue;
            }
            grid[start] = 2;
            stack.push((start % ww, start / ww));
            let mut n = 0u64;
            while let Some((x, y)) = stack.pop() {
                n += 1;
                for (dx, dy) in [
                    (-1, -1),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ] {
                    let (nx, ny) = (x.checked_add_signed(dx), y.checked_add_signed(dy));
                    let (Some(nx), Some(ny)) = (nx, ny) else {
                        continue;
                    };
                    if nx < ww && ny < wh && grid[ny * ww + nx] == 1 {
                        grid[ny * ww + nx] = 2;
                        stack.push((nx, ny));
                    }
                }
            }
            most = most.max(n);
        }
        self.points(most)
    }

    /// **The most concurrent flashing area**: over every one-second period,
    /// the densest window of the pixels that flash more than three times in
    /// it, by any of `spans`; with that window's largest connected piece and
    /// when that second starts.
    fn concurrent(&self, spans: &[&[Span]]) -> Worst {
        let mut worst = Worst {
            area: Area(0.0),
            contiguous: Area(0.0),
            second: 0.0,
        };
        let mut cuts: Vec<(Start, i32, Pixel)> = spans
            .iter()
            .flat_map(|s| s.iter())
            .flat_map(|s| [(s.from, 1, s.pixel), (s.to, -1, s.pixel)])
            .collect();
        if cuts.is_empty() {
            return worst;
        }
        cuts.sort_unstable_by_key(|c| (c.0, c.1));
        let (w, h) = self.sides();
        let mut live = vec![0u8; w * h];
        let mut on: Vec<Pixel> = Vec::new();
        let mut best: Option<(Densest, Vec<Pixel>)> = None;
        for group in cuts.chunk_by(|a, b| a.0 == b.0) {
            let mut grew = false;
            for &(_, d, p) in group {
                let i = usize::try_from(p.0).unwrap_or(0);
                if d > 0 {
                    live[i] = live[i].saturating_add(1);
                    grew |= live[i] == 1;
                } else {
                    live[i] = live[i].saturating_sub(1);
                }
            }
            if grew {
                on.clear();
                on.extend(
                    live.iter()
                        .enumerate()
                        .filter(|(_, n)| **n > 0)
                        .map(|(i, _)| Pixel(u32::try_from(i).unwrap_or(u32::MAX))),
                );
                let d = self.densest_at(&on);
                if best.as_ref().is_none_or(|(b, _)| d.area > b.area) {
                    worst.second = group[0].0.0 as f64 / f64::from(self.rate.0);
                    best = Some((d, on.clone()));
                }
            }
        }
        if let Some((d, pixels)) = best {
            worst.area = d.area;
            worst.contiguous = self.piece(&pixels, d.left, d.top);
        }
        worst
    }

    /// The count.
    pub fn finish(mut self) -> Report {
        // The legs still open when the run ended.
        for (i, z) in self.zig.iter().enumerate() {
            let pixel = Pixel(u32::try_from(i).unwrap_or(u32::MAX));
            let dir = match z.leg {
                Leg::Unset => continue,
                Leg::Rising => Dir::Up,
                Leg::Falling => Dir::Down,
            };
            if z.a.min(z.e) < DARKER {
                self.general.push(Event {
                    pixel,
                    at: z.et,
                    dir,
                });
            }
        }
        let rate = self.rate;
        let general = tally(core::mem::take(&mut self.general), rate);
        let red = tally(core::mem::take(&mut self.red), rate);
        let red_encoded = tally(core::mem::take(&mut self.red_encoded), rate);
        let series = |t: &Tallied| Series {
            transitions: t.transitions,
            flashes: t.flashes,
            chained: t.chained,
            over: self.concurrent(&[&t.over]).area,
            ever: self.densest(&t.ever),
        };
        let worst = self.concurrent(&[&general.over, &red.over]);
        let strict = self
            .concurrent(&[
                &general.over_chained,
                &red.over_chained,
                &red_encoded.over_chained,
            ])
            .area;
        Report {
            general: series(&general),
            red: series(&red),
            red_encoded: series(&red_encoded),
            area: worst.area,
            contiguous: worst.contiguous,
            second: worst.second,
            strict,
            apart: if self.apart.is_empty() {
                worst.area
            } else {
                self.concurrent(&[&self.kept(&general.over), &self.kept(&red.over)])
                    .area
            },
            apart_strict: if self.apart.is_empty() {
                strict
            } else {
                self.concurrent(&[
                    &self.kept(&general.over_chained),
                    &self.kept(&red.over_chained),
                    &self.kept(&red_encoded.over_chained),
                ])
                .area
            },
            threshold: Area(FIELD.0 * FIELD.1 * SHARE),
            frames: self.frame,
            translucent: self.translucent,
        }
    }
}

/// **A general-flash step**: the pixel's luminance `lum` on frame `t`. A turn
/// confirmed by a 0.10 return from the extremum closes a leg, which counts
/// when its darker end is below 0.80.
fn zig(z: &mut Zig, lum: f32, t: Frame, pixel: Pixel, out: &mut Vec<Event>) {
    let mut close = |a: f32, e: f32, et: Frame, dir: Dir| {
        if a.min(e) < DARKER {
            out.push(Event { pixel, at: et, dir });
        }
    };
    match z.leg {
        Leg::Unset => {
            if lum < z.a {
                z.a = lum;
                z.at = t;
            }
            if lum > z.e {
                z.e = lum;
                z.et = t;
            }
            if z.e - z.a >= CHANGE {
                if z.et > z.at {
                    z.leg = Leg::Rising;
                } else {
                    z.leg = Leg::Falling;
                    (z.a, z.at, z.e, z.et) = (z.e, z.et, z.a, z.at);
                }
            }
        }
        Leg::Rising => {
            if lum > z.e {
                z.e = lum;
                z.et = t;
            } else if z.e - lum >= CHANGE {
                close(z.a, z.e, z.et, Dir::Up);
                (z.a, z.at, z.e, z.et) = (z.e, z.et, lum, t);
                z.leg = Leg::Falling;
            }
        }
        Leg::Falling => {
            if lum < z.e {
                z.e = lum;
                z.et = t;
            } else if lum - z.e >= CHANGE {
                close(z.a, z.e, z.et, Dir::Down);
                (z.a, z.at, z.e, z.et) = (z.e, z.et, lum, t);
                z.leg = Leg::Rising;
            }
        }
    }
}

/// **A red-flash step**: the pixel seen as `s`, with `red` its redness on
/// the series' ratio. A state more than 0.2 from the last one a transition
/// left, where either is a saturated red, is a transition.
fn hue(h: &mut Hue, s: &Seen, red: Redness, t: Frame, pixel: Pixel, out: &mut Vec<Event>) {
    if h.red == Redness::Not && red == Redness::Not {
        return;
    }
    let d = ((s.u - h.u).powi(2) + (s.v - h.v).powi(2)).sqrt();
    if d <= STEP {
        return;
    }
    let dir = match (h.red, red) {
        (Redness::Not, Redness::Red) => Dir::Up,
        (Redness::Red, Redness::Not) => Dir::Down,
        _ => h.last.map_or(Dir::Up, Dir::opposite),
    };
    out.push(Event { pixel, at: t, dir });
    *h = Hue {
        u: s.u,
        v: s.v,
        red,
        last: Some(dir),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: u32 = 0xFF00_0000;
    const WHITE: u32 = 0xFFFF_FFFF;
    const PAGE: u32 = 0xFFEF_EFEF;

    /// A grey whose relative luminance is `l`, by the inverse of WCAG's curve.
    fn grey(l: f64) -> u32 {
        let v = if l <= 0.04045 / 12.92 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        let b = u32::from(u8::try_from((v * 255.0).round() as i64).unwrap_or(u8::MAX));
        0xFF00_0000 | (b << 16) | (b << 8) | b
    }

    /// A rectangle of the frame: left, top, width, height, in device pixels.
    #[derive(Clone, Copy)]
    struct Patch(u32, u32, u32, u32);

    impl Patch {
        fn holds(self, x: u32, y: u32) -> bool {
            x >= self.0 && x < self.0 + self.2 && y >= self.1 && y < self.1 + self.3
        }
    }

    /// A pixel word the ground is painted, `0xAARRGGBB`.
    #[derive(Clone, Copy)]
    struct Paint(u32);

    /// **A run of `seconds` at `rate`**, every pixel `ground` except inside
    /// each patch, which shows what its function gives for the frame.
    fn run(
        size: (Px, Px),
        scale: Scale,
        rate: Rate,
        seconds: f64,
        Paint(ground): Paint,
        patches: &[(Patch, &dyn Fn(Frame) -> u32)],
    ) -> Report {
        run_apart(size, scale, rate, seconds, Paint(ground), patches, &[])
    }

    /// The same, with `apart` left out of [`Report::apart`].
    fn run_apart(
        size: (Px, Px),
        scale: Scale,
        rate: Rate,
        seconds: f64,
        Paint(ground): Paint,
        patches: &[(Patch, &dyn Fn(Frame) -> u32)],
        apart: &[Patch],
    ) -> Report {
        let mut c = Census::new(size.0, size.1, scale, rate);
        for p in apart {
            c.exclude(Px::of(p.0), Px::of(p.1), Px::of(p.2), Px::of(p.3));
        }
        let n = (seconds * f64::from(rate.0)).round() as i64;
        let (w, h) = (size.0.value(), size.1.value());
        let mut buf = vec![ground; usize::try_from(w * h).unwrap()];
        for k in 0..u32::try_from(n).unwrap() {
            for y in 0..h {
                for x in 0..w {
                    let i = usize::try_from(y * w + x).unwrap();
                    buf[i] = patches
                        .iter()
                        .find(|(p, _)| p.holds(x, y))
                        .map_or(ground, |(_, f)| f(Frame(k)));
                }
            }
            c.frame(&buf);
        }
        c.finish()
    }

    /// A square wave at `hz` between `a` and `b`, half the period each, at
    /// `rate`: `a` first.
    fn square(hz: u32, rate: Rate, a: u32, b: u32) -> impl Fn(Frame) -> u32 {
        let half = rate.0 / (2 * hz);
        move |f: Frame| if (f.0 / half).is_multiple_of(2) { a } else { b }
    }

    const ONE: (Px, Px) = (Px::of(700), Px::of(300));
    const FPS: Rate = Rate(120);

    /// 30% of the 341 x 256 field: 200 x 131 is 26,200 of 87,296.
    const THIRTY: (u32, u32) = (200, 131);
    /// 20% of it: 160 x 109 is 17,440.
    const TWENTY: (u32, u32) = (160, 109);

    /// **A square flashing black and white at 4 Hz over 30% of a window
    /// fails**: four flashes in every second, eight transitions, over 26,200
    /// points squared. It straddles x = 341, so a counter that tiled the
    /// frame into fields rather than sliding one would see two halves under
    /// the threshold and pass it.
    #[test]
    fn a_four_hertz_square_over_thirty_percent_fails() {
        let f = square(4, FPS, BLACK, WHITE);
        let p = Patch(250, 40, THIRTY.0, THIRTY.1);
        let r = run(ONE, Scale(1), FPS, 2.0, Paint(BLACK), &[(p, &f)]);
        assert_eq!(r.general.flashes, Flashes(4), "{r:?}");
        assert_eq!(r.general.transitions, Transitions(8), "{r:?}");
        assert_eq!(r.area, Area(26_200.0), "{r:?}");
        assert_eq!(r.outcome(), Outcome::Fails, "{r:?}");
        assert_eq!(r.red.flashes, Flashes(0), "grey is never red: {r:?}");
        assert_eq!(r.translucent, Tally::default());
    }

    /// **The same square at 2 Hz passes**: two flashes a second, at any area.
    #[test]
    fn a_two_hertz_square_passes() {
        let f = square(2, FPS, BLACK, WHITE);
        let p = Patch(250, 40, THIRTY.0, THIRTY.1);
        let r = run(ONE, Scale(1), FPS, 2.0, Paint(BLACK), &[(p, &f)]);
        assert_eq!(r.general.flashes, Flashes(2), "{r:?}");
        assert_eq!(r.area, Area(0.0), "{r:?}");
        assert_eq!(r.outcome(), Outcome::Passes, "{r:?}");
        // Four transitions a second: the chained reading's three flashes,
        // still not more than three.
        assert_eq!(r.strict_outcome(), Outcome::Passes, "{r:?}");
    }

    /// **A 4 Hz square over 20% of a window passes on area**: it flashes four
    /// times a second, over 17,440 points squared, under 21,824.
    #[test]
    fn a_four_hertz_square_over_twenty_percent_passes() {
        let f = square(4, FPS, BLACK, WHITE);
        let p = Patch(250, 40, TWENTY.0, TWENTY.1);
        let r = run(ONE, Scale(1), FPS, 2.0, Paint(BLACK), &[(p, &f)]);
        assert_eq!(r.general.flashes, Flashes(4), "{r:?}");
        assert_eq!(r.area, Area(17_440.0), "{r:?}");
        assert_eq!(r.outcome(), Outcome::Passes, "{r:?}");
    }

    /// **Red and cyan at one luminance flash red, and not generally**: pure
    /// red's relative luminance is 0.2126, and `#008E8E`'s 0.2131, so no
    /// change reaches 0.10, while red's R/(R+G+B) is 1 and the two are 0.36
    /// apart in u'v'. At 4 Hz over 30% of the window that fails, on the red
    /// count alone.
    #[test]
    fn red_and_cyan_at_one_luminance_count_as_red_flashes() {
        let f = square(4, FPS, 0xFFFF_0000, 0xFF00_8E8E);
        let p = Patch(250, 40, THIRTY.0, THIRTY.1);
        let r = run(ONE, Scale(1), FPS, 2.0, Paint(BLACK), &[(p, &f)]);
        assert_eq!(r.general.transitions, Transitions(0), "{r:?}");
        assert_eq!(r.general.flashes, Flashes(0), "{r:?}");
        assert_eq!(r.red.flashes, Flashes(4), "{r:?}");
        assert_eq!(r.red_encoded.flashes, Flashes(4), "{r:?}");
        assert_eq!(r.area, Area(26_200.0), "{r:?}");
        assert_eq!(r.outcome(), Outcome::Fails, "{r:?}");
    }

    /// **A 4 Hz sine whose every frame step is under 0.10 fails**: luminance
    /// 0.05 to 0.60 at 120 frames a second moves at most 0.058 a frame, so a
    /// frame-to-frame counter sees nothing; measured from the last extremum
    /// it is eight changes of 0.55 a second.
    #[test]
    fn a_sine_with_steps_under_the_change_still_flashes() {
        let f = |k: Frame| {
            let t = f64::from(k.0) / f64::from(FPS.0);
            grey(0.325 - 0.275 * (2.0 * core::f64::consts::PI * 4.0 * t).cos())
        };
        // The premise: no frame steps 0.10.
        let lum = |p: u32| {
            let c = f64::from(u8::try_from(p & 0xFF).unwrap()) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let most = (0..240)
            .map(|k| (lum(f(Frame(k + 1))) - lum(f(Frame(k)))).abs())
            .fold(0.0, f64::max);
        assert!(most < 0.07, "a frame steps {most}");
        let p = Patch(250, 40, THIRTY.0, THIRTY.1);
        let r = run(ONE, Scale(1), FPS, 2.0, Paint(BLACK), &[(p, &f)]);
        assert_eq!(r.general.flashes, Flashes(4), "{r:?}");
        assert_eq!(r.outcome(), Outcome::Fails, "{r:?}");
    }

    /// **White against `#EFEFEF` never flashes generally**: the darker state,
    /// 0.863, is not below 0.80, however fast or large.
    #[test]
    fn a_change_whose_darker_state_is_bright_is_not_a_flash() {
        let f = square(4, FPS, WHITE, PAGE);
        let p = Patch(250, 40, THIRTY.0, THIRTY.1);
        let r = run(ONE, Scale(1), FPS, 2.0, Paint(PAGE), &[(p, &f)]);
        assert_eq!(r.general.flashes, Flashes(0), "{r:?}");
        assert_eq!(r.general.transitions, Transitions(0), "{r:?}");
        assert_eq!(r.outcome(), Outcome::Passes, "{r:?}");
        assert_eq!(r.strict_outcome(), Outcome::Passes, "{r:?}");
    }

    /// **The second slides**: four flashes at 6.7 Hz, 0.6 s of them centred
    /// on the one-second mark, are four in one second, which a counter that
    /// binned whole seconds would split two and two.
    #[test]
    fn four_flashes_across_a_second_mark_are_four_in_one_second() {
        // Frames 84 to 155, 0.70 s to 1.30 s: eight half-periods of nine
        // frames, four of them each side of frame 120.
        let f = |k: Frame| {
            if (84..156).contains(&k.0) && ((k.0 - 84) / 9).is_multiple_of(2) {
                WHITE
            } else {
                BLACK
            }
        };
        let p = Patch(250, 40, THIRTY.0, THIRTY.1);
        let r = run(ONE, Scale(1), FPS, 2.0, Paint(BLACK), &[(p, &f)]);
        assert_eq!(r.general.flashes, Flashes(4), "{r:?}");
        assert_eq!(r.outcome(), Outcome::Fails, "{r:?}");
    }

    /// **Areas that flash in different seconds are not summed**: two patches
    /// of 15% each at 4 Hz, one in the first second and one in the third,
    /// make 15% at any one time, though 30% flashed over the run.
    #[test]
    fn areas_flashing_in_different_seconds_are_not_concurrent() {
        let sq = square(4, FPS, BLACK, WHITE);
        let early = |k: Frame| if k.0 < 120 { sq(k) } else { BLACK };
        let late = |k: Frame| if k.0 >= 240 { sq(k) } else { BLACK };
        // 100 x 131 is 13,100 each.
        let a = Patch(20, 40, 100, 131);
        let b = Patch(150, 40, 100, 131);
        let r = run(
            ONE,
            Scale(1),
            FPS,
            3.5,
            Paint(BLACK),
            &[(a, &early), (b, &late)],
        );
        assert_eq!(r.general.flashes, Flashes(4), "{r:?}");
        assert_eq!(r.area, Area(13_100.0), "{r:?}");
        assert_eq!(r.general.ever, Area(26_200.0), "{r:?}");
        assert_eq!(r.outcome(), Outcome::Passes, "{r:?}");
    }

    /// **Pieces apart in one window are summed, and the largest is reported
    /// beside them**: two patches of 13,100 at 4 Hz, a column apart, make
    /// 26,200 concurrently and fail, while no one piece is more than 13,100.
    #[test]
    fn pieces_apart_are_summed_and_the_largest_is_reported() {
        let sq = square(4, FPS, BLACK, WHITE);
        let a = Patch(20, 40, 100, 131);
        let b = Patch(121, 40, 100, 131);
        let r = run(ONE, Scale(1), FPS, 2.0, Paint(BLACK), &[(a, &sq), (b, &sq)]);
        assert_eq!(r.area, Area(26_200.0), "{r:?}");
        assert_eq!(r.contiguous, Area(13_100.0), "{r:?}");
        assert_eq!(r.outcome(), Outcome::Fails, "{r:?}");
        // One patch is one piece.
        let one = run(ONE, Scale(1), FPS, 2.0, Paint(BLACK), &[(a, &sq)]);
        assert_eq!(one.contiguous, one.area, "{one:?}");
    }

    /// **Pixels left out are left out of the apart area only**: a patch of
    /// 17,440 and a cursor-sized one of 6,000 beside it, both at 4 Hz, make
    /// 23,440 as drawn, which fails, and 17,440 with the small one excluded,
    /// which passes -- on both readings. The frames are the same either way.
    #[test]
    fn excluded_pixels_leave_the_apart_area_and_stay_in_the_verdicts() {
        let sq = square(4, FPS, BLACK, WHITE);
        let trail = Patch(20, 40, TWENTY.0, TWENTY.1);
        let cursor = Patch(181, 40, 100, 60);
        let r = run_apart(
            ONE,
            Scale(1),
            FPS,
            2.0,
            Paint(BLACK),
            &[(trail, &sq), (cursor, &sq)],
            &[cursor],
        );
        assert_eq!(r.area, Area(23_440.0), "{r:?}");
        assert_eq!(r.outcome(), Outcome::Fails, "{r:?}");
        assert_eq!(r.apart, Area(17_440.0), "{r:?}");
        assert_eq!(r.apart_outcome(), Outcome::Passes, "{r:?}");
        assert_eq!(r.apart_strict, Area(17_440.0), "{r:?}");
        assert_eq!(r.apart_strict_outcome(), Outcome::Passes, "{r:?}");
        // Nothing excluded: the apart area is the verdict's.
        let all = run(
            ONE,
            Scale(1),
            FPS,
            2.0,
            Paint(BLACK),
            &[(trail, &sq), (cursor, &sq)],
        );
        assert_eq!(
            (all.apart, all.apart_strict),
            (all.area, all.strict),
            "{all:?}"
        );
    }

    /// **At two pixels a point the field is 682 x 512 and the area is in
    /// points**: 30% of it, 400 x 262 device pixels, is 26,200 points
    /// squared, and fails.
    #[test]
    fn at_two_pixels_a_point_the_area_is_in_points() {
        let f = square(4, FPS, BLACK, WHITE);
        let p = Patch(100, 60, 400, 262);
        let r = run(
            (Px::of(800), Px::of(600)),
            Scale(2),
            FPS,
            2.0,
            Paint(BLACK),
            &[(p, &f)],
        );
        assert_eq!(r.area, Area(26_200.0), "{r:?}");
        assert_eq!(r.threshold, Area(21_824.0));
        assert_eq!(r.outcome(), Outcome::Fails, "{r:?}");
    }

    /// **Three flashes a second at five transitions**: a pulse train whose
    /// second holds five transitions is two disjoint flashes and four chained
    /// ones -- the reading the strict verdict takes -- and only the strict
    /// verdict fails it.
    #[test]
    fn the_chained_reading_is_the_strict_one() {
        // 2.5 Hz: 24 frames a half-period at 120.
        let f = |k: Frame| {
            if (k.0 / 24).is_multiple_of(2) {
                BLACK
            } else {
                WHITE
            }
        };
        let p = Patch(250, 40, THIRTY.0, THIRTY.1);
        let r = run(ONE, Scale(1), FPS, 3.0, Paint(BLACK), &[(p, &f)]);
        assert_eq!(r.general.transitions, Transitions(5), "{r:?}");
        assert_eq!(r.general.flashes, Flashes(2), "{r:?}");
        assert_eq!(r.general.chained, Flashes(4), "{r:?}");
        assert_eq!(r.outcome(), Outcome::Passes, "{r:?}");
        assert_eq!(r.strict_outcome(), Outcome::Fails, "{r:?}");
    }
}
