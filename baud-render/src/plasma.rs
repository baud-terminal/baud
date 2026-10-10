//! **The plasma trail**: a translucent tube of ionised gas filling kitty's
//! collapsing quad, three thin filaments braided along it and crawling into
//! the cursor, and ions peeling off the path in cyclotron spirals. It starves
//! rather than burns: the tube, the sheath and the filaments dim together as
//! `(1 - k)^2` and the filaments thin, where the fire burns out from the tail.
//! The default trail, the owner's call of 2026-09-29, replacing the fire.
//! Designed in the effect-trail research (the plasma spec and its critique),
//! and built with the critique's fixes:
//!
//! * **Nothing is drawn ahead of the cursor.** The braid swings to zero
//!   ahead of the head, so without a gate its three filaments lay on the
//!   centre line there and poked a spike out of the cursor's leading edge;
//!   the filaments fade in over the first 0.3 cell behind the head.
//! * **The braid's axis is the move the quad is making**, [`Vfx::axis`], which
//!   the driver carries from the segment it last took. The spec's axis from
//!   the quad's centroid to the hole fell back to the ignition's direction
//!   every time the corners landed -- the common case -- and popped.
//! * **The braid's width is the cursor's, across the axis**: a bar or an
//!   underline cursor collapses it to one line rather than erasing it.
//! * **The filament gain is a constant.** WCAG 2.3.1's area clause is per
//!   341x256 window, so a braid that stays inside the quad cannot exceed it
//!   whatever the move's length, and the spec's length cap never engaged.
//!
//! In the cursor's colour, intensity its opacity; GPU only, as every effect.

use crate::effect::{self, Hash, Ignition, Particle, Vfx};
use crate::trail::Pt;

/// Seconds the body lasts: the envelope `(1 - k)^2` is zero here, so the
/// quad the driver drops at [`end`] leaves nothing visible behind.
pub const LIFE: f32 = 0.55;

/// Cells past the quad the sheath reaches: `SHEATH * e^-DECAY` is under half
/// a step of 255, so the fragment function rejects a pixel this far out.
pub const REACH: f32 = 1.0;

/// Opacity the tube adds inside the quad, over the sheath.
pub const TUBE: f32 = 0.08;

/// The sheath's opacity at the quad's edge, and inside it.
pub const SHEATH: f32 = 0.20;

/// The sheath's e-fold rate a cell, at ignition; it grows as the plasma
/// cools, so the sheath contracts toward the tube.
pub const DECAY: f32 = 5.0;

/// The filaments' gain.
pub const FILAMENT: f32 = 0.95;

/// How often a brightness knot passes a fixed point on a filament, a second.
pub const KNOT: f32 = 2.5;

/// The effect's own numbers this frame, `k` in `EffectU`: the braid's half
/// width across the axis in cells, the axis, then the constants above.
pub fn params(vfx: &Vfx) -> [f32; effect::PARAMS] {
    let a = vfx.axis;
    let h = vfx.emitter.hole;
    let unit = vfx.ignition.unit.max(1.0);
    let half = 0.5 * (a.y.abs() * (h.right - h.left) + a.x.abs() * (h.bottom - h.top)) / unit;
    [half, a.x, a.y, TUBE, SHEATH, DECAY, FILAMENT, KNOT]
}

/// Ions a unit of path, and the least and most an ignition sheds.
const IONS_PER_UNIT: f32 = 3.0;

/// Seconds over which the emitter sheds ions along the path.
const SWEEP: f32 = 0.10;

/// An ion's drag, a second, and its turn rate, radians a second -- two
/// turns, the sense set by its charge.
const DRAG: f32 = 2.5;
const TURN: f32 = 2.0 * core::f32::consts::TAU;

/// **Seconds from ignition to when the plasma is out**: its life, or its
/// last ion's death.
pub fn end(ignition: &Ignition) -> f32 {
    shed(ignition)
        .map(|h| {
            let i = Ion::draw(h);
            i.born + i.life
        })
        .fold(LIFE, f32::max)
}

fn shed(ignition: &Ignition) -> impl Iterator<Item = Hash> + use<> {
    let n = (ignition.len / ignition.unit.max(1.0) * IONS_PER_UNIT).clamp(8.0, 64.0);
    let seed = ignition.seed.0;
    (0..u32::try_from(n.round() as i64).unwrap_or(8))
        .map(move |i| Hash(seed ^ i.wrapping_mul(0x9E37_79B9)).next())
}

/// An ion's draws: where along the path it is born and when, how long it
/// lives, and the hashes its place, charge, speed and size come from.
struct Ion {
    along: f32,
    born: f32,
    life: f32,
    h: [Hash; 7],
}

impl Ion {
    fn draw(h0: Hash) -> Ion {
        let mut h = [h0; 7];
        for i in 1..7 {
            h[i] = h[i - 1].next();
        }
        let along = h0.unit();
        Ion {
            along,
            born: along * SWEEP,
            life: 0.25 + 0.30 * h[1].unit(),
            h,
        }
    }

    /// The ion's charge: the sense it curls in.
    fn charge(&self) -> f32 {
        if self.h[2].unit() < 0.5 { 1.0 } else { -1.0 }
    }

    /// Where it starts and how fast, in pixels and pixels a second.
    fn launch(&self, f: &Ignition) -> (Pt, Pt) {
        let u = f.unit;
        let perp = Pt {
            x: -f.dir.y,
            y: f.dir.x,
        };
        let [_, _, _, h3, h4, h5, _] = self.h;
        let side = (h3.unit() - 0.5) * 0.6 * u;
        let p0 = Pt {
            x: f.tail.x + f.dir.x * f.len * self.along + perp.x * side,
            y: f.tail.y + f.dir.y * f.len * self.along + perp.y * side,
        };
        let q = self.charge();
        let speed = (1.8 + 2.4 * h4.unit()) * u;
        let turn = (h5.unit() - 0.5) * core::f32::consts::FRAC_PI_2;
        let (s, c) = turn.sin_cos();
        let (dx, dy) = (q * perp.x, q * perp.y);
        let carry = (f.len / SWEEP).min(40.0 * u) * 0.03;
        let v0 = Pt {
            x: speed * (dx * c - dy * s) + f.dir.x * carry,
            y: speed * (dx * s + dy * c) + f.dir.y * carry,
        };
        (p0, v0)
    }

    /// **Where it is `a` seconds after its birth**: its velocity decays and
    /// turns, `v(a) = v0 e^(-D a) R(w a)`, integrated in closed form --
    /// `p0 + v0 (e^((-D + i w) a) - 1) / (-D + i w)` as a complex number.
    fn centre(&self, f: &Ignition, a: f32) -> Pt {
        let (p0, v0) = self.launch(f);
        let w = self.charge() * TURN;
        let e = (-DRAG * a).exp();
        let (s, c) = (w * a).sin_cos();
        let den = DRAG * DRAG + w * w;
        let mr = (DRAG * (1.0 - e * c) + w * e * s) / den;
        let mi = (w * (1.0 - e * c) - DRAG * e * s) / den;
        Pt {
            x: p0.x + mr * v0.x - mi * v0.y,
            y: p0.y + mi * v0.x + mr * v0.y,
        }
    }

    fn at(&self, f: &Ignition) -> Option<Particle> {
        let a = f.age - self.born;
        if a < 0.0 || a > self.life {
            return None;
        }
        let heat = 1.0 - a / self.life;
        Some(Particle {
            centre: self.centre(f, a),
            // A floor of two pixels: under it the soft disc lights too little
            // of its record to be seen at 1x.
            radius: (f.unit * (0.05 + 0.04 * self.h[6].unit()) * (0.6 + 0.4 * heat)).max(2.0),
            glow: 0.85 * heat * heat.sqrt(),
        })
    }
}

/// Every ion alive at the ignition's age.
pub fn ions(ignition: &Ignition) -> impl Iterator<Item = Particle> + '_ {
    shed(ignition).filter_map(move |h| Ion::draw(h).at(ignition))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::tests::{CELL, cell, ignition};
    use crate::effect::{Effect, Seed};
    use crate::trail::{Shape, Smear};

    /// **The closed form is the motion it names**: a fine numeric
    /// integration of `v0 e^(-D a) R(w a)` lands where the closed form puts
    /// the ion, over its whole life, and at the start it leaves at `v0`.
    #[test]
    fn an_ions_spiral_is_its_integrated_velocity() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(5));
        for h in shed(&f).take(12) {
            let ion = Ion::draw(h);
            let (p0, v0) = ion.launch(&f);
            let w = ion.charge() * TURN;
            let (mut x, mut y) = (p0.x as f64, p0.y as f64);
            let steps = 200_000u32;
            let dt = f64::from(ion.life) / f64::from(steps);
            for i in 0..steps {
                let t = (f64::from(i) + 0.5) * dt;
                let (e, (s, c)) = ((-f64::from(DRAG) * t).exp(), (f64::from(w) * t).sin_cos());
                x += (f64::from(v0.x) * c - f64::from(v0.y) * s) * e * dt;
                y += (f64::from(v0.x) * s + f64::from(v0.y) * c) * e * dt;
            }
            let at = ion.centre(&f, ion.life);
            assert!(
                (f64::from(at.x) - x).abs() < 0.01 && (f64::from(at.y) - y).abs() < 0.01,
                "closed form {at:?}, integrated ({x}, {y})"
            );
            let a = 1e-3;
            let early = ion.centre(&f, a);
            assert!(
                (early.x - (p0.x + v0.x * a)).abs() < 0.01
                    && (early.y - (p0.y + v0.y * a)).abs() < 0.01,
                "it leaves at v0"
            );
        }
    }

    /// **Ions leave both ways and curl both ways**: half one charge, half
    /// the other, each thrown off its own side of the path, and each lives
    /// between its birth and its death only.
    #[test]
    fn ions_leave_both_sides_and_live_their_span() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(21));
        let (mut plus, mut minus) = (0, 0);
        for h in shed(&f) {
            let ion = Ion::draw(h);
            assert!(ion.born >= 0.0 && ion.born <= SWEEP);
            assert!((0.25..=0.55).contains(&ion.life));
            assert_eq!(ion.at(&f.at(ion.born - 0.001)), None);
            assert_eq!(ion.at(&f.at(ion.born + ion.life + 0.001)), None);
            let (_, v0) = ion.launch(&f);
            // The path runs along +x, so a positive charge goes to +y.
            if ion.charge() > 0.0 {
                plus += 1;
                assert!(v0.y > 0.0, "a positive ion leaves to its side");
            } else {
                minus += 1;
                assert!(v0.y < 0.0, "a negative ion leaves to the other");
            }
            let born = ion.at(&f.at(ion.born)).expect("at its birth");
            let late = ion.at(&f.at(ion.born + ion.life * 0.8)).expect("late");
            assert!(late.glow < born.glow, "fading");
            assert!(late.radius >= 2.0, "never below the floor");
        }
        assert_eq!(plus + minus, 45, "three a unit of a fifteen-unit path");
        assert!(plus > 16 && minus > 16, "both charges: {plus} and {minus}");
    }

    /// **The plasma is out when its last ion dies**, and never before its
    /// life.
    #[test]
    fn the_plasma_is_out_when_its_last_ion_dies() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(4));
        let out = end(&f);
        assert!((LIFE..=0.65).contains(&out), "out at {out}");
        assert!(ions(&f.at(out - 0.01)).count() > 0 || out == LIFE);
        assert_eq!(ions(&f.at(out + 0.001)).count(), 0);
        assert_eq!(Effect::Plasma.end(&f), out);
    }

    /// **The braid lies along the move and is as wide as the cursor across
    /// it**: the axis is the one the driver hands, and a bar cursor's hole,
    /// two pixels wide, narrows a vertical move's braid to a line.
    #[test]
    fn the_braid_follows_the_axis_and_the_cursors_width() {
        let to = cell(40.0, 5.0);
        let mut vfx = Vfx {
            effect: Effect::Plasma,
            emitter: Smear {
                corners: to.corners(),
                hole: to,
                shape: Shape::Quad,
            },
            ignition: ignition(cell(10.0, 5.0), to, Seed(1)),
            axis: Pt { x: 1.0, y: 0.0 },
        };
        let k = params(&vfx);
        assert_eq!((k[1], k[2]), (1.0, 0.0));
        assert!(
            (k[0] - 0.5).abs() < 1e-6,
            "half the cell's height: {}",
            k[0]
        );
        vfx.axis = Pt { x: 0.0, y: 1.0 };
        assert!((params(&vfx)[0] - 0.5 * CELL.0 / CELL.1).abs() < 1e-6);
        vfx.emitter.hole.right = vfx.emitter.hole.left + 2.0;
        assert!(
            (params(&vfx)[0] - 1.0 / CELL.1).abs() < 1e-6,
            "a bar's width"
        );
        assert_eq!(&params(&vfx)[3..], &[TUBE, SHEATH, DECAY, FILAMENT, KNOT]);
    }

    /// **The sheath is under half a step at the reach**, which is why the
    /// fragment function may reject a pixel past it.
    #[test]
    fn the_sheath_is_gone_at_the_reach() {
        assert!(SHEATH * (-DECAY * REACH).exp() < 0.5 / 255.0);
    }
}
