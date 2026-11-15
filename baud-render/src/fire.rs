//! **The fire trail**: a procedural flame rising off the quad trail's
//! collapsing quad, and embers the emitter sheds along its path. One effect of the family in [`crate::effect`], which
//! holds what every effect shares: the ignition, the records and the uniform.
//! This is the fire's own: its numbers, its life and its embers.
//!
//! **The flame is the cursor's colour** -- the one the quad trail is drawn in
//! -- and heat is its opacity: a hot core is the cursor, a tongue's tip and
//! the halo are translucent.
//!
//! **Rust owns the numbers.** The flame's height, rise and drift ride in the
//! uniform's own numbers ([`PARAMS`]), so the shader and the embers built
//! here read one spelling of each.

use crate::det::Det;
use crate::effect::{self, Hash, Ignition, Particle, Vfx, smoothstep};
use crate::trail::Pt;

/// Seconds the flame burns, from ignition: the tail burns out first and the
/// head, at the cursor, last.
pub const LIFE: f32 = 0.45;

/// Cells a tongue of flame can climb above the emitter.
pub const HEIGHT: f32 = 1.6;

/// Cells from the emitter past which the flame draws nothing: a tongue lifts
/// a sample at most [`HEIGHT`] cells, and the halo is under half a step of
/// 255 by then. `fire_fragment` rejects a pixel this far out before any noise,
/// and the records cover exactly this reach.
pub const REACH: f32 = HEIGHT + 0.4;

/// Cells a second the flame's pattern climbs.
pub const RISE: f32 = 2.2;

/// Cells a second the pattern trails behind the move.
pub const DRIFT: f32 = 0.6;

/// The fire's own numbers as `fire_fragment` reads them, `k` in `EffectU`:
/// the flame's height, rise and drift, and how far its path has ridden a
/// smooth scroll ([`Ignition::rode`]), which the noise is anchored on.
pub fn params(ig: &Ignition) -> [f32; effect::PARAMS] {
    [HEIGHT, RISE, DRIFT, ig.rode, 0.0, 0.0, 0.0, 0.0]
}

/// `fx_fbm`: four octaves of [`effect::noise`], each rotated and at a
/// detuned double of the last's scale.
fn fbm(mut x: f32, mut y: f32, seed: Hash) -> f32 {
    let (mut t, mut a) = (0.0f32, 0.5f32);
    for o in 0..4u32 {
        t += a * effect::noise(x, y, Hash(seed.0.wrapping_add(o.wrapping_mul(0x9E37_79B9))));
        (x, y) = ((0.8 * x - 0.6 * y) * 2.02, (0.6 * x + 0.8 * y) * 2.02);
        a *= 0.5;
    }
    t
}

/// **The flame's light at `p`**, `fire_flame`: tongues where noise lifts a
/// sample down into the quad, cooling with distance from it and burning out
/// from the tail, and a halo on the page -- the opacity the cursor's colour
/// is drawn at, before the record's alpha.
pub fn light(vfx: &Vfx, p: Pt) -> f32 {
    let (e, f) = (vfx.emitter, vfx.ignition);
    if e.hole.contains(p) {
        return 0.0;
    }
    let k = f.age / LIFE;
    if k >= 1.0 {
        return 0.0;
    }
    let c = f.unit;
    let d0 = effect::distance(p, &e.corners) / c;
    if d0 > REACH {
        return 0.0;
    }
    // The noise is anchored on the ride, `k[3]`, as the shader reads it.
    let rode = params(&f)[3];
    let n = fbm(
        p.x / c * 2.6 - f.dir.x * (f.age * DRIFT),
        ((p.y - rode) / c + f.age * RISE) * 0.7 - f.dir.y * (f.age * DRIFT),
        Hash(f.seed.0),
    );
    let lift = HEIGHT * c * smoothstep(0.25, 0.85, n);
    let d = effect::distance(
        Pt {
            x: p.x,
            y: p.y + lift,
        },
        &e.corners,
    ) / c;
    let body = (1.0 - smoothstep(-0.10, 0.20, d)).max(0.8 * (1.0 - smoothstep(-0.25, 0.05, d0)));
    let cool = (d0.max(0.0) / HEIGHT).clamp(0.0, 1.0);
    let u = (((p.x - f.tail.x) * f.dir.x + (p.y - f.tail.y) * f.dir.y) / f.len.max(1.0))
        .clamp(0.0, 1.0);
    let burn = smoothstep(k - 0.15, k + 0.35, u) * (1.0 - k * k);
    let heat = (body * (1.0 - 0.85 * cool) * (0.50 + 0.60 * n) * burn).clamp(0.0, 1.0);
    let halo = 0.12 * (-2.5 * d0.max(0.0)).det_exp() * burn;
    (smoothstep(0.05, 0.95, heat) + halo).clamp(0.0, 1.0)
}

/// **Seconds from ignition to when the fire is out**: the flame's life, or
/// the last ember's death if that is later.
pub fn end(ignition: &Ignition) -> f32 {
    shed(ignition)
        .map(|h| {
            let e = Ember::draw(h);
            e.born + e.life
        })
        .fold(LIFE, f32::max)
}

/// The embers the path sheds, each as its first hash: eight a unit of path
/// -- the cell's height, so about 128 for a thirty-column hop, what the
/// research rendered the look with -- between 16 and 160, ember `i` drawing
/// from the seed with the index spread by the golden ratio's bits, so
/// neighbours are unrelated.
fn shed(ignition: &Ignition) -> impl Iterator<Item = Hash> + use<> {
    let n = (ignition.len / ignition.unit.max(1.0) * EMBERS_PER_UNIT).clamp(16.0, 160.0);
    let seed = ignition.seed.0;
    (0..u32::try_from(n.round() as i64).unwrap_or(16))
        .map(move |i| Hash(seed ^ i.wrapping_mul(0x9E37_79B9)).next())
}

/// Embers a unit of path sheds.
const EMBERS_PER_UNIT: f32 = 8.0;

/// Seconds the emitter takes to pass the path, which is when an ember is
/// born at its place along it.
const SWEEP: f32 = 0.10;

/// An ember's drag a second, and its buoyancy, cells a second squared
/// upward: it leaves fast, slows, and floats.
const DRAG: f32 = 3.0;
const BUOYANCY: f32 = 14.0;

/// **One ember's draw from the ignition's hashes**: where along the path it
/// is born and when, how long it lives, and the rest of its hashes.
struct Ember {
    along: f32,
    born: f32,
    life: f32,
    h: [Hash; 5],
}

impl Ember {
    fn draw(h0: Hash) -> Ember {
        let (h1, h2, h3, h4) = {
            let h1 = h0.next();
            let h2 = h1.next();
            let h3 = h2.next();
            (h1, h2, h3, h3.next())
        };
        let along = h0.unit();
        Ember {
            along,
            born: along * SWEEP,
            life: 0.30 + 0.45 * h1.unit(),
            h: [h0, h1, h2, h3, h4],
        }
    }
}

/// **The embers alive at the ignition's age**, each in closed form: born
/// where the emitter passes, thrown up and sideways, slowed by drag, lifted
/// by buoyancy, fluttering, `p = p0 + v0 (1 - e^-ka)/k + (g/k)(a - (1 -
/// e^-ka)/k)` -- the research's proposal B, a pure function of the seed and
/// the age.
pub fn embers(ignition: &Ignition) -> impl Iterator<Item = Particle> + '_ {
    shed(ignition).filter_map(move |h| Ember::draw(h).at(ignition))
}

impl Ember {
    /// This ember at the ignition's age, or none before it is born or after
    /// it dies.
    fn at(&self, flame: &Ignition) -> Option<Particle> {
        let a = flame.age - self.born;
        if a < 0.0 || a > self.life {
            return None;
        }
        let u = flame.unit;
        let perp = Pt {
            x: -flame.dir.y,
            y: flame.dir.x,
        };
        let carry = (flame.len / SWEEP).min(40.0 * u) * 0.05;
        let [_, h1, h2, h3, h4] = self.h;
        let side = (h2.unit() - 0.5) * u * 0.8;
        let p0 = Pt {
            x: flame.tail.x + flame.dir.x * flame.len * self.along + perp.x * side,
            y: flame.tail.y + flame.dir.y * flame.len * self.along + perp.y * side,
        };
        let v0 = Pt {
            x: (h3.unit() - 0.5) * 2.0 * u + flame.dir.x * carry,
            y: -(3.0 + 5.0 * h4.unit()) * u + flame.dir.y * carry,
        };
        let s = (1.0 - (-DRAG * a).det_exp()) / DRAG;
        let lift = BUOYANCY * u / DRAG * (a - s);
        let phase = core::f32::consts::TAU * Hash(h1.0 ^ 0x5bd1_e995).unit();
        let heat = 1.0 - a / self.life;
        Some(Particle {
            centre: Pt {
                x: p0.x + v0.x * s + 0.35 * u * (11.0 * a + phase).det_sin(),
                y: p0.y + v0.y * s - lift,
            },
            radius: u * (0.05 + 0.06 * h2.unit()) * (0.6 + 0.4 * heat),
            glow: heat * (0.55 + 0.45 * heat),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::Seed;
    use crate::effect::tests::{CELL, cell, ignition};

    /// **An ember is born where the emitter passes and floats up.** Born
    /// within [`SWEEP`] of the ignition, at its place along the path, within
    /// half a cell of it either side; by half its life it has risen at least
    /// 0.4 of a cell, whatever way it was thrown -- the slowest, shortest-lived
    /// ember rises about half a cell by then; after its life it is gone.
    #[test]
    fn an_ember_is_born_where_the_emitter_passes_and_floats_up() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(11));
        let mut seen = 0;
        for h in shed(&f) {
            let e = Ember::draw(h);
            assert!(e.born >= 0.0 && e.born <= SWEEP, "born at {}", e.born);
            assert!((0.30..=0.75).contains(&e.life), "lives {}", e.life);
            assert_eq!(e.at(&f.at(e.born - 0.001)), None, "before its birth");
            assert_eq!(
                e.at(&f.at(e.born + e.life + 0.001)),
                None,
                "after its death"
            );
            let at_birth = e.at(&f.at(e.born)).expect("at its birth");
            let path = Pt {
                x: f.tail.x + f.dir.x * f.len * e.along,
                y: f.tail.y + f.dir.y * f.len * e.along,
            };
            assert!(
                (at_birth.centre.x - path.x).abs() <= 0.4 * CELL.1 + 0.001
                    && (at_birth.centre.y - path.y).abs() <= 0.4 * CELL.1 + 0.001,
                "born at {:?}, the path at {path:?}",
                at_birth.centre
            );
            let later = e
                .at(&f.at(e.born + e.life * 0.5))
                .expect("at half its life");
            assert!(
                later.centre.y < at_birth.centre.y - 0.4 * CELL.1,
                "risen: {later:?} from {at_birth:?}"
            );
            assert!(
                later.glow < at_birth.glow && later.radius < at_birth.radius,
                "cooling"
            );
            seen += 1;
        }
        assert_eq!(seen, 120, "eight embers a unit of a fifteen-unit path");
    }

    /// **The fire is out when its last ember dies**, and not before: the
    /// flame's life or the last death, whichever is later.
    #[test]
    fn the_fire_is_out_when_its_last_ember_dies() {
        for seed in [Seed(1), Seed(2), Seed(99)] {
            let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), seed);
            let end = end(&f);
            assert!((LIFE..=SWEEP + 0.75).contains(&end), "out at {end}");
            if end > LIFE {
                assert!(
                    embers(&f.at(end - 0.001)).next().is_some(),
                    "an ember at the end"
                );
            }
            assert!(embers(&f.at(end + 0.001)).next().is_none(), "none after it");
        }
    }

    /// **Eight embers a unit of path, between 16 and 160**, and the same
    /// seed sheds them alike where another does not.
    #[test]
    fn embers_are_eight_a_unit_and_the_seed_decides_them() {
        let count = |cells: f32| {
            shed(&ignition(
                cell(0.0, 0.0),
                cell(cells * CELL.1 / CELL.0, 0.0),
                Seed(1),
            ))
            .count()
        };
        assert_eq!(count(1.0), 16);
        assert_eq!(count(10.0), 80);
        assert_eq!(count(100.0), 160);
        let shed = |seed: Seed| -> Vec<Particle> {
            embers(&ignition(cell(10.0, 5.0), cell(40.0, 5.0), seed).at(0.2)).collect()
        };
        assert_eq!(shed(Seed(4)), shed(Seed(4)));
        assert_ne!(shed(Seed(4)), shed(Seed(5)));
    }
}
