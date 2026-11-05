//! **iTerm2's fireworks**, `OSC 1337 ; RequestAttention=fireworks`:
//! "fireworks explode at the cursor's location"
//! (`[[source: iterm2-escape-codes]]`). The page says no more, so what is
//! drawn is what iTerm2 draws, read at its source
//! (`[[source: iterm2-964a75d-findcursorview]]`): a Core Animation emitter
//! at the cursor's centre whose invisible cells, born four a second, each
//! throw yellow, red and white stars in every direction, a fast family
//! and a slow one, spinning, growing and fading out over a second, with a
//! hole round the cursor where nothing is drawn; the window they are drawn
//! in is taken down a second after the last request, its alpha animated to
//! nothing over Core Animation's default quarter second.
//!
//! **Two of iTerm2's numbers are not baud's, for WCAG 2.3.1.** Drawn with
//! every number iTerm2's ([`ITERM2`]), the show flashes more than three
//! times a second over more of a 341 by 256 point field than the 21,824
//! square points the general flash threshold allows
//! (`baud-bench/tests/fireworks_flashes.rs`). baud's ([`BAUD`]) throws the
//! slow stars at 40 points a second rather than 100 and half as many fast
//! ones, ten a second of each colour rather than twenty; every other
//! number is iTerm2's. Each star is drawn over the frame in order, where
//! iTerm2's emitter adds them before its window is laid over the
//! terminal, and inside baud's own window, where iTerm2's spans the
//! screens.
//!
//! **Under Reduce Motion nothing moves**: the show's frame half a second in
//! is held still and faded out over the show's length ([`Form::Still`]),
//! the fade Apple's guidance puts in place of motion
//! (`[[source: apple-hig-accessibility]]`).
//!
//! **Everything is a pure function of the show**: the driver hands a
//! [`Show`] -- so many seconds in, when its hold ends, where the cursor was
//! as each burst was born and where it is now, and the backing scale --
//! and this derives the stars and the records each path draws them from,
//! as [`crate::effect`] derives a trail's. No clock and no state, so the
//! pixel oracle compares two GPU arms on a frame and the CPU path draws it
//! by the same model ([`coverage`], [`hole`]).

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::det::Det;
use crate::effect::{Hash, Seed};
use crate::instance::{Instance, Screen};
use crate::trail::Pt;
use baud_domain::Chan;
use baud_font::Px;

/// **Bursts born a second**: the emitter's one cell, `setBirthRate:4`,
/// whose particles draw nothing (`setScale:0`, no contents) and each emit
/// the stars.
pub const BURSTS: f32 = 4.0;

/// A burst's life, seconds, and its range about it: `setLifetime:0.75`,
/// `setLifetimeRange:0.25`, "a mean value and a range about the mean"
/// (`[[source: macos-sdk-caemittercell-h]]`).
pub const BURST_LIFE: (f32, f32) = (0.75, 0.25);

/// **A family of stars**: how many a burst throws a second of each
/// colour, and how fast, points a second, a tenth of it the range
/// (`subcellWithImageNumber:birthRate:velocity:delay:`, `velocityRange:v *
/// 0.1`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Family {
    pub rate: f32,
    pub speed: f32,
}

/// iTerm2's fast stars: `birthRate:b/5 velocity:v`, `b` 100 and `v` 1000.
pub const FAST: Family = Family {
    rate: 20.0,
    speed: 1000.0,
};

/// iTerm2's slow stars: `birthRate:b velocity:v/10`.
pub const SLOW: Family = Family {
    rate: 100.0,
    speed: 100.0,
};

/// **The three stars' colours**, `FindCursorCell1`, `2` and `3`: a
/// yellow, a red and a white star, each opaque inside its edge.
pub const COLOURS: [Argb; 3] = [Argb(0xFFFF_FF00), Argb(0xFFFF_0000), Argb(0xFFFF_FFFF)];

/// A star's life, seconds, and its range: `lifetime = 1`,
/// `setLifetimeRange: lifetime * 0.3`. Its alpha falls by one a second
/// (`setAlphaSpeed:-1 / lifetime`) from opaque, so one that lives past a
/// second is clear from then on.
pub const STAR_LIFE: (f32, f32) = (1.0, 0.3);

/// A star's scale, its range and its growth a second: `setScale:0`,
/// `setScaleRange:0.1`, `setScaleSpeed:0.3`, of an image [`IMAGE`] points
/// a side.
pub const SCALE: (f32, f32, f32) = (0.0, 0.1, 0.3);

/// The star images' side, points: each is 200 by 200.
pub const IMAGE: f32 = 200.0;

/// A star's spin and its range, radians a second: `setSpin:M_PI * 6`,
/// `setSpinRange:M_PI * 2`.
pub const SPIN: (f32, f32) = (core::f32::consts::PI * 6.0, core::f32::consts::PI * 2.0);

/// **The star in its image**, as fractions of the side: five points, the
/// tips this far from its centre and the notches between them this far,
/// one tip straight up, its centre this far below the image's. Measured
/// on the 200-pixel images: the notches 41 pixels out, the centroid 8
/// below the middle, and the tips 98.7 out where their edges meet -- the
/// images' half coverage crosses them at 97, short of a sharp point, and
/// the area, 11,888 pixels, is a regular five-pointed star's of these
/// radii.
pub const TIP: f32 = 0.4935;
pub const NOTCH: f32 = 0.205;
pub const DROP: f32 = 0.04;

/// **The hole round the cursor**, points: the mask's circle of radius 20
/// about the cursor's centre, inside which nothing is drawn.
pub const HOLE: f32 = 20.0;

/// **Seconds the show holds after the last request**, `kFindCursorHoldTime`:
/// its teardown timer, restarted by each request while it shows.
pub const HOLD: f32 = 1.0;

/// **Seconds the window takes to go**: its alpha animated to nothing by
/// the animator proxy at the default duration, "1/4s"
/// (`[[source: macos-sdk-catransaction-h]]`), with no timing function,
/// "linear pacing" (`[[source: macos-sdk-caanimation-h]]`). The emitter
/// runs on under it.
pub const FADE: f32 = 0.25;

/// **Seconds into the show the still form shows**, under Reduce Motion:
/// the frame the look was chosen on.
pub const STILL: f32 = 0.5;

/// The longest a burst has stars on screen after its birth: its longest
/// life, then a star thrown as it dies, clear a second later.
const BURST_REACH: f32 = BURST_LIFE.0 + BURST_LIFE.1 + 1.0;

/// **Bursts whose origins a show keeps**, a ring: those born in the last
/// [`BURST_REACH`] seconds, at [`BURSTS`] a second, with room to spare.
pub const ORIGINS: usize = 16;

/// **What a show draws**: the numbers baud's look changes from iTerm2's.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    /// The fast family and the slow one.
    pub families: [Family; 2],
}

/// **iTerm2's look**, every number its source's: past the general flash
/// threshold, so baud draws it only to count it.
pub const ITERM2: Look = Look {
    families: [FAST, SLOW],
};

/// **baud's look**: the slow stars at 40 points a second and half as many
/// fast ones.
pub const BAUD: Look = Look {
    families: [
        Family {
            rate: 10.0,
            speed: 1000.0,
        },
        Family {
            rate: 100.0,
            speed: 40.0,
        },
    ],
};

/// **How a show moves**: as iTerm2's, or held still under Reduce Motion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    Moving,
    /// The show's frame [`STILL`] seconds in, every star where it is then,
    /// faded out linearly from whole at the start to nothing at the end.
    Still,
}

/// **A show for a frame**, as the driver hands it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Show {
    /// Seconds since the show began: the first request's frame.
    pub age: f32,
    /// Seconds from the show's start to the end of its hold: [`HOLD`] after
    /// the last request.
    pub hold: f32,
    /// The cursor's centre, frame pixels, as each burst was born: burst `k`
    /// at `k % ORIGINS`. iTerm2 moves the emitter to the cursor on each
    /// draw (`drawingHelperUpdateFindCursorView`), so a burst comes from
    /// where the cursor was when it was born, and stays there.
    pub origins: [Pt; ORIGINS],
    /// The cursor's centre now, frame pixels: the hole's.
    pub hole: Pt,
    /// Device pixels a point.
    pub scale: f32,
    pub seed: Seed,
    pub look: Look,
    pub form: Form,
}

/// **The shows a frame draws**: at most two, the older first -- a request
/// while one fades starts another, as iTerm2's opens a new window while
/// the last one's alpha falls.
pub type Shows = [Option<Show>; 2];

/// **What tells one frame's fireworks from the last's**: each show's age
/// and hole, which move on every frame a show is up; the renderer keeps the
/// shows themselves beside it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Stamp([Option<(f32, Pt)>; 2]);

impl Stamp {
    pub fn of(shows: &Shows) -> Stamp {
        Stamp(shows.map(|s| s.map(|s| (s.age, s.hole))))
    }
}

/// **One star at the show's age**: its image's centre in frame pixels, its
/// image's side in pixels, its turn, its colour and its opacity.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Star {
    pub centre: Pt,
    pub side: f32,
    /// Its rotation, in turns, counter-clockwise on screen: Core
    /// Animation's positive spin in the view's upward y.
    pub turn: f32,
    pub colour: Argb,
    pub alpha: f32,
}

/// **A burst's place in a show**, from 0: born `k / BURSTS` seconds in,
/// its origin the show's at `k` modulo [`ORIGINS`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Burst(pub u32);

impl Burst {
    /// When the burst is born, seconds into the show.
    pub fn born(self) -> f32 {
        f32::from(u16::try_from(self.0).unwrap_or(u16::MAX)) / BURSTS
    }

    /// **How many bursts are born by `age`**, as the place of the first one
    /// that is not: those at `k / BURSTS <= age`.
    pub fn by(age: f32) -> Burst {
        if age < 0.0 {
            return Burst(0);
        }
        let k = u32::try_from((age * BURSTS).floor() as i64).unwrap_or(0);
        Burst(k.saturating_add(1))
    }

    /// The burst after this one.
    pub fn next(self) -> Burst {
        Burst(self.0.saturating_add(1))
    }

    /// Its origin in a show's ring.
    pub fn of(self, origins: &[Pt; ORIGINS]) -> Pt {
        origins[usize::try_from(self.0).unwrap_or(0) % ORIGINS]
    }

    /// Its origin set in a show's ring.
    pub fn set(self, origins: &mut [Pt; ORIGINS], at: Pt) {
        origins[usize::try_from(self.0).unwrap_or(0) % ORIGINS] = at;
    }
}

/// **A star's birth in its family and colour**, from 0: thrown `phase + n
/// period` seconds after its burst.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Birth(u32);

impl Birth {
    fn seconds(self) -> f32 {
        f32::from(u16::try_from(self.0).unwrap_or(u16::MAX))
    }
}

/// A float as a birth, rounded toward zero; none below zero.
fn birth(v: f32) -> Birth {
    Birth(u32::try_from(v.max(0.0) as i64).unwrap_or(0))
}

impl Show {
    /// Seconds from the show's start to when it is gone: the window's fade
    /// over.
    pub fn end(&self) -> f32 {
        self.hold + FADE
    }

    /// **How much of the show is drawn**: moving, whole through the hold,
    /// then falling to nothing over [`FADE`], linearly, as the window's
    /// alpha; still, falling linearly from the start to the end.
    pub fn window(&self) -> f32 {
        match self.form {
            Form::Moving => (1.0 - (self.age - self.hold) / FADE).clamp(0.0, 1.0),
            Form::Still => (1.0 - self.age / self.end()).clamp(0.0, 1.0),
        }
    }

    /// **Every star on screen at the show's age**, burst by burst, family
    /// by family, colour by colour, oldest first: those born and not yet
    /// dead or clear, while the window is up. A still show's are those of
    /// its frame [`STILL`] in, at its opacity.
    pub fn stars(&self, mut each: impl FnMut(Star)) {
        let window = self.window();
        if window <= 0.0 || self.age < 0.0 {
            return;
        }
        let age = match self.form {
            Form::Moving => self.age,
            Form::Still => STILL,
        };
        for k in (0..Burst::by(age.min(self.end())).0).map(Burst) {
            let t0 = k.born();
            if age - t0 > BURST_REACH {
                continue;
            }
            let h = Hash(self.seed.0 ^ k.0.wrapping_mul(0x9E37_79B9)).next();
            let life = BURST_LIFE.0 + BURST_LIFE.1 * (2.0 * h.unit() - 1.0);
            let origin = k.of(&self.origins);
            for (f, fam) in self.look.families.iter().enumerate() {
                let period = 1.0 / fam.rate;
                for (c, &colour) in COLOURS.iter().enumerate() {
                    let key = Hash(
                        h.0.wrapping_add(
                            u32::try_from(f * 3 + c)
                                .unwrap_or(0)
                                .wrapping_mul(0x85EB_CA6B),
                        ),
                    );
                    // The colours' births interleave: each starts a third
                    // of a period after the last.
                    let phase = period * f32::from(u8::try_from(c).unwrap_or(0)) / 3.0;
                    star_births(age, t0, life, period, phase, |n, a| {
                        if let Some(s) = star(self, fam, key, n, a, origin) {
                            each(Star {
                                colour,
                                alpha: s.alpha * window,
                                ..s
                            });
                        }
                    });
                }
            }
        }
    }
}

/// Each star of a family and colour a burst born at `t0` and living
/// `life` has thrown by `age` and that is not yet clear: its birth `n` and
/// its age. Births at `t0 + phase + n period` while the burst lives; those
/// thrown more than a second ago are clear.
fn star_births(
    age: f32,
    t0: f32,
    life: f32,
    period: f32,
    phase: f32,
    mut each: impl FnMut(Birth, f32),
) {
    let first = ((age - 1.0 - t0 - phase) / period).ceil().max(0.0);
    let last = ((age.min(t0 + life) - t0 - phase) / period).floor();
    if last < first {
        return;
    }
    for n in (birth(first).0..=birth(last).0).map(Birth) {
        let a = age - (t0 + phase + n.seconds() * period);
        if a >= 0.0 {
            each(n, a);
        }
    }
}

/// **Star `n` of a family `a` seconds after its birth**, in no colour yet:
/// its life, speed, direction, scale and spin drawn from its hash; none
/// once dead or clear. A negative scale draws the image turned half round.
fn star(show: &Show, fam: &Family, key: Hash, n: Birth, a: f32, origin: Pt) -> Option<Star> {
    let s = Hash(key.0 ^ n.0.wrapping_mul(0xC2B2_AE35)).next();
    let u = [s, s.next(), s.next().next(), s.next().next().next()];
    let r = |h: Hash| 2.0 * h.unit() - 1.0;
    let life = STAR_LIFE.0 + STAR_LIFE.1 * r(u[0]);
    let alpha = 1.0 - a;
    if a >= life || alpha <= 0.0 {
        return None;
    }
    let speed = fam.speed * (1.0 + 0.1 * r(u[1]));
    let (sn, cs) = (core::f32::consts::TAU * u[2].unit()).det_sin_cos();
    let scale = SCALE.0 + SCALE.1 * r(u[2].next()) + SCALE.2 * a;
    let spin = SPIN.0 + SPIN.1 * r(u[3]);
    let go = speed * a * show.scale;
    let flip = if scale < 0.0 { 0.5 } else { 0.0 };
    Some(Star {
        centre: Pt {
            x: origin.x + cs * go,
            y: origin.y + sn * go,
        },
        side: scale.abs() * IMAGE * show.scale,
        turn: (spin * a / core::f32::consts::TAU + flip).fract(),
        colour: COLOURS[0],
        alpha,
    })
}

/// The record's half side over the image's side: the star reaches
/// [`TIP`] + [`DROP`] from the image's centre at any turn.
pub const REACH: f32 = TIP + DROP;

/// **A star's record's side in pixels**: odd, so its centre is a pixel's,
/// and wide enough for its tips at any turn and a pixel of edge past them.
pub fn record_side(image: f32) -> Tex {
    let half = (REACH * image + 1.0).ceil().max(1.0);
    Tex(u16::try_from(half as i64).unwrap_or(u16::MAX / 2) * 2 + 1)
}

/// Sixteenths of a pixel a record's `v` counts a star's image side in.
pub const SIDE_STEPS: f32 = 16.0;

/// A star's image side as a record's `v`.
pub fn side_word(image: f32) -> Tex {
    Tex(u16::try_from((image * SIDE_STEPS).round().clamp(0.0, 65_535.0) as i64).unwrap_or(0))
}

/// A record's `v` back to the image side both paths draw the star at.
pub fn image_side(word: Tex) -> f32 {
    f32::from(word.0) / SIDE_STEPS
}

/// A turn as a record's `u`, 1/65536 of a turn a step.
pub fn turn_word(turn: f32) -> Tex {
    let steps = ((turn.fract() + 1.0).fract() * 65536.0).round() as i64;
    Tex(u16::try_from(steps & 0xFFFF).unwrap_or(0))
}

/// **A record's `u` back to the turn's cosine and sine**, as the CPU path
/// turns by. The GPU's `fireworks_vertex` reads the same step from the
/// record and computes its own `precise::sincos`, which has no table. The
/// two can differ in the last bit, and
/// `the_fireworks_draw_alike_on_every_arm` allows one step on a few pixels
/// for it (one was measured).
pub fn turned(word: Tex) -> (f32, f32) {
    let th = f32::from(word.0) * (core::f32::consts::TAU / 65536.0);
    let (s, c) = th.det_sin_cos();
    (c, s)
}

/// The steps of a turn a record's `u` counts.
pub const TURNS: u32 = 65_536;

/// **A star's coverage at `p`**, pixels from its record's centre, y down,
/// its image `image` pixels a side and turned by `(cos, sin)`: the
/// five-pointed star of [`TIP`] and [`NOTCH`], one tip up before the turn,
/// its centre [`DROP`] below the image's, anti-aliased over a pixel
/// across its edge.
pub fn coverage(p: Pt, image: f32, (c, s): (f32, f32)) -> f32 {
    // Into the image's frame, y down: turned back clockwise on screen,
    // then from the star's centre, and y up.
    let x = p.x * c - p.y * s;
    let y = p.x * s + p.y * c - DROP * image;
    let d = star_distance(Pt { x, y: -y }, TIP * image, NOTCH * image);
    (0.5 - d).clamp(0.0, 1.0)
}

/// **How much of a pixel at `p` the mask leaves**, frame pixels: none
/// within `radius` of the cursor's centre `at`, all from a pixel out,
/// anti-aliased between.
pub fn hole(p: Pt, at: Pt, radius: f32) -> f32 {
    let (dx, dy) = (p.x - at.x, p.y - at.y);
    ((dx * dx + dy * dy).sqrt() - radius + 0.5).clamp(0.0, 1.0)
}

/// cos and sin of 18 and 54 degrees: the star's mirror lines either side
/// of the tip that points right of up, from its centre.
const MIRROR_18: (f32, f32) = (0.951_056_5, 0.309_017);
const MIRROR_54: (f32, f32) = (0.587_785_24, 0.809_017);

/// **The signed distance from `p` to a five-pointed star**, negative
/// inside, y up, a tip at `(0, tip)` and the notches `notch` out: `p`
/// folded by the star's mirror lines into the wedge from that tip to the
/// notch at 54 degrees, then measured to the edge between them. The
/// fold-then-segment form follows Inigo Quilez's sdStar5() among his 2D
/// distance functions, the geometry spelled over again rather than the
/// code copied, as the ufo effect's is.
pub fn star_distance(p: Pt, tip: f32, notch: f32) -> f32 {
    let mut q = Pt {
        x: p.x.abs(),
        y: p.y,
    };
    // Across the line at 18 degrees, then the one at 54: a point below
    // either is reflected above it, by the line's normal (-sin, cos).
    for (cs, sn) in [MIRROR_18, MIRROR_54] {
        let d = -sn * q.x + cs * q.y;
        if d < 0.0 {
            q = Pt {
                x: q.x + 2.0 * d * sn,
                y: q.y - 2.0 * d * cs,
            };
        }
    }
    q.x = q.x.abs();
    // The edge from the tip to the notch at 54 degrees, and the side of it
    // the centre is on.
    let (ex, ey) = (notch * MIRROR_54.0, notch * MIRROR_54.1 - tip);
    let (px, py) = (q.x, q.y - tip);
    let h = ((px * ex + py * ey) / (ex * ex + ey * ey)).clamp(0.0, 1.0);
    let (dx, dy) = (px - ex * h, py - ey * h);
    let d = (dx * dx + dy * dy).sqrt();
    // The centre is on the side where the cross product is negative.
    if ex * py - ey * px < 0.0 { -d } else { d }
}

/// **The records a path draws a show with**, appended to `out`: one a
/// star, its odd-sided bounds centred on the pixel its centre is in, in
/// its colour at its opacity, its turn in `u` and its image's side in `v`;
/// none for a star off the frame or under a step of opacity. The rows the
/// records span are what the show dirties.
pub fn records(show: &Show, frame: (Px, Px), out: &mut Vec<Instance>) {
    let side = |p: Px| f32::from(u16::try_from(p.value()).unwrap_or(u16::MAX));
    let (fw, fh) = (side(frame.0), side(frame.1));
    show.stars(|s| {
        let alpha = Chan::from_unit(s.alpha);
        let v = side_word(s.side);
        if alpha.get() == 0 || v.0 == 0 {
            return;
        }
        let w = record_side(image_side(v)).0;
        let half = f32::from(w / 2);
        let (x, y) = (s.centre.x.floor() - half, s.centre.y.floor() - half);
        let wf = f32::from(w);
        if x >= fw || y >= fh || x + wf <= 0.0 || y + wf <= 0.0 {
            return;
        }
        out.push(Instance {
            x: Screen(x.clamp(-32_000.0, 32_000.0) as i16),
            y: Screen(y.clamp(-32_000.0, 32_000.0) as i16),
            w: Tex(w),
            h: Tex(w),
            u: turn_word(s.turn),
            v,
            color: s.colour.with_alpha(alpha),
            kind: Kind::Particle,
            curve: baud_domain::contrast::CurveRow::default(),
        });
    });
}

/// **A star record's centre**, frame pixels: the centre of its middle
/// pixel.
pub fn centre(r: &Instance) -> Pt {
    let half = f32::from(r.w.0 / 2);
    Pt {
        x: f32::from(r.x.0) + half + 0.5,
        y: f32::from(r.y.0) + half + 0.5,
    }
}

/// The floats in the uniform the GPU's fireworks fragment function reads.
pub const UNIFORM_FLOATS: usize = 8;

/// **The uniform the GPU's fireworks fragment function reads**: where the
/// frame sits in the target, then the hole's centre and radius in frame
/// pixels.
pub fn uniform(origin: [f32; 2], hole: Pt, radius: f32) -> [f32; UNIFORM_FLOATS] {
    [origin[0], origin[1], hole.x, hole.y, radius, 0.0, 0.0, 0.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(age: f32) -> Show {
        let at = Pt { x: 400.5, y: 300.5 };
        Show {
            age,
            hold: HOLD,
            origins: [at; ORIGINS],
            hole: at,
            scale: 1.0,
            seed: Seed(9),
            look: BAUD,
            form: Form::Moving,
        }
    }

    /// **The star is the images' star**: its tips [`TIP`] of the side out,
    /// its notches [`NOTCH`], a tip straight up, so the coverage three
    /// pixels inside each tip and one inside each notch is whole, and a
    /// pixel past either none; its area within a percent of the measured
    /// 11,888 pixels of a 200-pixel image.
    #[test]
    fn the_star_is_the_images_star() {
        let up = (1.0, 0.0);
        let side = 200.0;
        let centre_y = DROP * side;
        let at = |deg: f32, r: f32| {
            let (s, c) = deg.to_radians().det_sin_cos();
            Pt {
                x: r * s,
                y: centre_y - r * c,
            }
        };
        for k in 0..5u8 {
            let tip = 72.0 * f32::from(k);
            let notch = tip + 36.0;
            assert_eq!(
                coverage(at(tip, TIP * side - 3.0), side, up),
                1.0,
                "tip {k}"
            );
            assert_eq!(
                coverage(at(tip, TIP * side + 1.0), side, up),
                0.0,
                "tip {k}"
            );
            assert_eq!(coverage(at(notch, NOTCH * side - 1.0), side, up), 1.0);
            assert_eq!(coverage(at(notch, NOTCH * side + 1.0), side, up), 0.0);
        }
        let mut area = 0.0;
        for y in 0..200u8 {
            for x in 0..200u8 {
                let p = Pt {
                    x: f32::from(x) + 0.5 - 100.0,
                    y: f32::from(y) + 0.5 - 100.0,
                };
                area += coverage(p, side, up);
            }
        }
        assert!((area - 11_888.0f32).abs() < 119.0, "area {area}");
    }

    /// **A turn turns the star counter-clockwise**: a quarter turn puts the
    /// top tip on the left, and none on the right.
    #[test]
    fn a_turn_turns_the_star_counter_clockwise() {
        let side = 200.0;
        let q = turned(turn_word(0.25));
        // The star's centre is DROP below the image's before the turn, so
        // DROP to the right of it after a quarter turn.
        let left = Pt {
            x: DROP * side - (TIP * side - 3.0),
            y: 0.0,
        };
        assert_eq!(coverage(left, side, q), 1.0);
        let right = Pt {
            x: DROP * side + TIP * side - 1.0,
            y: 0.0,
        };
        assert_eq!(coverage(right, side, q), 0.0, "no tip points right");
    }

    /// **The show is drawn, and ends**: stars from the first frame, every
    /// one within the fast family's reach of the cursor, and none once the
    /// window's fade is over; while it holds, whole; through the fade,
    /// falling.
    #[test]
    fn the_show_is_drawn_and_ends() {
        let mut n = 0;
        show(0.0).stars(|_| n += 1);
        assert!(n > 0, "a star on the first frame");
        for age in [0.1f32, 0.5, 1.0, 1.2] {
            let s = show(age);
            let mut far = 0.0f32;
            s.stars(|st| {
                let (dx, dy) = (st.centre.x - 400.5, st.centre.y - 300.5);
                far = far.max((dx * dx + dy * dy).sqrt());
            });
            assert!(far <= 1100.0 * age + 1.0, "{age}: {far}");
        }
        assert_eq!(show(HOLD).window(), 1.0);
        assert!(show(HOLD + FADE / 2.0).window() < 1.0);
        let mut left = 0;
        show(HOLD + FADE).stars(|_| left += 1);
        assert_eq!(left, 0, "gone at the end");
    }

    /// **Still, nothing moves**: under [`Form::Still`] every frame draws the
    /// same stars at the same places, sizes and turns, only fainter, and
    /// none at the end.
    #[test]
    fn a_still_show_does_not_move() {
        let still = |age: f32| {
            let mut v = Vec::new();
            Show {
                form: Form::Still,
                ..show(age)
            }
            .stars(|s| v.push(s));
            v
        };
        let (a, b) = (still(0.1), still(0.9));
        assert!(!a.is_empty());
        assert_eq!(a.len(), b.len());
        for (p, q) in a.iter().zip(&b) {
            assert_eq!((p.centre, p.side, p.turn), (q.centre, q.side, q.turn));
            assert!(q.alpha < p.alpha);
        }
        assert!(still(HOLD + FADE).is_empty());
    }
}
