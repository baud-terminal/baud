//! The quad, sweep and effect trails' driver: when a cursor move starts one,
//! when it retargets, and which frames it owes.
//!
//! **The geometry lives in `baud_render::trail`**: kitty's four corners
//! chasing the cursor's, each with its own time constant, or sfte's tail --
//! the cursor's rectangle, every corner on one constant -- in closed form.
//! What lives here is which moves become a trail, the same for both, after
//! two sources:
//!
//! * **A move of two cells or fewer, from rest, snaps.** kitty's
//!   `cursor_trail_start_threshold 2` (`[[source: kitty-cursor-trail]]`
//!   66-70, `[[source: kitty-options-definition]]` 482): typing moves the
//!   cursor a cell at a time, and a trail per keystroke is noise. A move
//!   while a trail is in flight retargets it whatever its size, as kitty's
//!   does.
//! * **The cursor has to rest at its new place before the trail follows
//!   it** -- kitty's rule: its target moves only once `cursor_trail` ms
//!   have passed since the program last moved the cursor
//!   (`[[source: kitty-cursor-trail]]` 171). The number is sfte's, 10 ms by
//!   its own "sensible default" (`[[source: sfte]]` 13891); sfte's own rule
//!   is the other way round, a move trailing only when the one before it
//!   was that long ago (38180-38186). A program streaming output moves the
//!   cursor on every read; kitty's rule waits for it to stop rather than
//!   chasing it.
//!
//! An effect -- the fire and its family -- rides kitty's quad, the fire's
//! flame rising off its corners, and adds an **ignition**: when it was lit,
//! its seed and the path, from which `baud_render::effect` derives the effect
//! and its particles. It outlives the chase, burning out after the corners
//! land, and two rules keep it from flashing:
//!
//! * **At most three ignitions a second.** A long move within
//!   [`REIGNITE`] of the last ignition retargets the quad and keeps the flame
//!   that is already burning, rather than lighting a new one. It limits
//!   ignitions, not flashes: counted on the frames the trails draw, one pixel
//!   flashed up to 23 times a second under it, and at 32 pt the fire and the
//!   orb fail WCAG 2.3.1 on a light ground; at the default 13 pt every effect
//!   passes, by the area clause alone (docs/record.md, "The cursor trails'
//!   flashes, counted on the frames they draw").
//! * **Typing never ignites**: a move of two cells or fewer snaps, as it does
//!   for the quad, and a burning flame follows the cursor it snapped to.
//!
//! The clock is the driver's -- the display link's timestamp -- and a move
//! waiting out its dwell owes the tick one frame when the dwell runs out
//! ([`Chase::owes`]), because after a lone keystroke's echo nothing else
//! would draw one. The trail is backdated to when the cursor was first seen at
//! its new place, so the frame that starts it is already that far along.

use core::time::Duration;

use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{Pt, Rect, Segment, Shape, Smear, TrailLook};

use crate::bell::Moment;
use crate::caret::Owes;

/// How long the cursor must rest at a new place before the trail follows it:
/// sfte's default.
pub const DWELL: Duration = Duration::from_millis(10);

/// A move of at most this many cells on each axis, from rest, snaps: kitty's
/// default start threshold.
pub const THRESHOLD: f32 = 2.0;

/// The least time between two ignitions of an effect, so it lights at most
/// three times a second.
pub const REIGNITE: Duration = Duration::from_millis(334);

/// **Scratch prototype, not baud's code**: which retarget rule `take` runs.
/// 0 the shipped rule; 1 a long move in flight inside REIGNITE puts the
/// effect out and snaps; 2 it lands the quad on the cursor at once and keeps
/// the flame burning there; 3 at most three drawn sweeps in any second, the
/// rest snap and put the effect out.
pub static VARIANT: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
fn variant() -> u8 {
    VARIANT.load(core::sync::atomic::Ordering::Relaxed)
}

/// Scratch, variant 4: the ignition gate at 1 s instead of `REIGNITE`.
fn reignite() -> Duration {
    if variant() == 4 { Duration::from_millis(1000) } else { REIGNITE }
}


/// Which trail the driver runs: a smear -- the quad or the sweep -- or an
/// effect, which rides the quad.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Smear(Shape),
    Effect(Effect),
}

/// An effect's ignition while it burns: when it was lit, the ignition at age
/// 0, and the seconds until it is out.
#[derive(Clone, Copy)]
struct Burning<M> {
    at: M,
    ignition: Ignition,
    end: f32,
}

/// Whether the move in flight still rides a smooth scroll, and how far it
/// has, pixels, y down: at most [`THRESHOLD`] rows, then it folds in on the
/// fast constant and rides no more -- the owner's call of 2026-09-27, so a
/// trail in flight when output starts streaming lands rather than hanging a
/// row or more above the cursor for as long as the stream lasts.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Carry {
    Riding(f32),
    Folded,
}

/// A trail's state between frames.
pub struct Chase<M> {
    /// Which trail it draws, and so which constants a long move takes.
    style: Style,
    /// The rectangle the corners rest at or chase: the cursor's, as last
    /// taken. `None` until the cursor is first seen, and after a snap.
    target: Option<Rect>,
    /// The move in flight and the moment it started.
    moving: Option<(Segment, M)>,
    /// How far that move has ridden a smooth scroll.
    carry: Carry,
    /// The cursor seen somewhere else, and when it was first seen there,
    /// waiting out the dwell.
    pending: Option<(Rect, M)>,
    /// An effect's ignition while it burns.
    fire: Option<Burning<M>>,
    /// When the last ignition was lit, which outlives it: a lightning bolt
    /// is out in 0.32 s, inside [`REIGNITE`], so a guard read off the live
    /// ignition would let a fourth bolt a second through.
    last_lit: Option<M>,
    /// The seed the next ignition takes: one an ignition, so no two in a
    /// session burn alike.
    next_seed: Seed,
    /// The direction of the segment last taken, which an effect's frame
    /// hands on as [`Vfx::axis`]: it turns when the quad turns and stays when
    /// the corners land, where the quad's own shape says nothing.
    axis: Pt,
    /// Scratch: when the last drawn sweeps started, for variant 3.
    sweeps: [Option<M>; 3],
}

impl<M: Moment> Chase<M> {
    pub fn new(style: Style) -> Chase<M> {
        Chase {
            style,
            target: None,
            moving: None,
            carry: Carry::Riding(0.0),
            pending: None,
            fire: None,
            last_lit: None,
            next_seed: Seed(1),
            axis: Pt { x: 1.0, y: 0.0 },
            sweeps: [None; 3],
        }
    }

    /// **A frame's trail**: the cursor as this frame draws it, or where a
    /// program placed the one it hid (`Term::placed_cursor`) -- `None` when it
    /// is hidden unplaced or the window is looking back -- at `now`, on cells
    /// `cell` pixels wide and tall, and what the frame draws.
    pub fn frame(&mut self, cursor: Option<Rect>, now: M, cell: (f32, f32)) -> Option<TrailLook> {
        let Some(rect) = cursor else {
            // Nothing to arrive at: no trail, no move waiting and no fire,
            // and the place it was seen stays where a trail would start from.
            self.moving = None;
            self.pending = None;
            self.fire = None;
            return None;
        };
        let Some(target) = self.target else {
            self.target = Some(rect);
            return None;
        };
        if rect == target {
            self.pending = None;
        } else if near(target, rect, cell) && self.folded(now, rect, cell) {
            // A short move snaps -- and at once, because a snap draws nothing
            // and so needs neither the dwell nor the frame it would owe --
            // from rest, or from a trail that has already folded to within
            // the threshold of the cursor: a key held after a jump steps
            // faster than a step folds, and would otherwise keep a frame on
            // every tick until it is let go.
            self.target = Some(rect);
            self.pending = None;
            self.moving = None;
        } else {
            match self.pending {
                Some((p, since)) if p == rect => {
                    if now.elapsed_since(since) >= DWELL {
                        self.take(target, rect, since, now, cell);
                    }
                }
                _ => self.pending = Some((rect, now)),
            }
        }
        // The corners this frame, while a move is in flight.
        let corners = self.moving.and_then(|(seg, t0)| {
            let t = now.elapsed_since(t0).as_secs_f32();
            (t < seg.lasts()).then(|| seg.at(t))
        });
        if corners.is_none() {
            self.moving = None;
        }
        // The hole is the cursor as this frame draws it, which a move still
        // inside its dwell has already left the segment's target for: the
        // trail never covers the cursor.
        match self.style {
            Style::Smear(shape) => corners.map(|corners| {
                TrailLook::Smear(Smear {
                    corners,
                    hole: rect,
                    shape,
                })
            }),
            Style::Effect(effect) => {
                let Some(ig) = self.fire else {
                    // Nothing burns: a long move the three-a-second guard
                    // refused after the last ignition went out. It draws
                    // nothing, so the quad it would ride owes no frames.
                    self.moving = None;
                    return None;
                };
                let age = now.elapsed_since(ig.at).as_secs_f32();
                if age >= ig.end {
                    // Out: the flame and every ember, and the quad it rode
                    // with them.
                    self.fire = None;
                    self.moving = None;
                    return None;
                }
                // Once the corners land the flame burns on at the cursor.
                Some(TrailLook::Vfx(Vfx {
                    effect,
                    emitter: Smear {
                        corners: corners.unwrap_or_else(|| rect.corners()),
                        hole: rect,
                        shape: Shape::Quad,
                    },
                    ignition: ig.ignition.at(age),
                    axis: self.axis,
                }))
            }
        }
    }

    /// The cursor has rested at `rect` long enough: follow it.
    fn take(&mut self, target: Rect, rect: Rect, since: M, now: M, cell: (f32, f32)) {
        self.pending = None;
        self.target = Some(rect);
        self.carry = Carry::Riding(0.0);
        match self.moving {
            // In flight: retarget from where the corners are now, so they
            // do not jump -- and a short move on the fast constant, so typing
            // after a jump folds the trail in rather than keeping it alive.
            Some((seg, t0)) => {
                let from = seg.at(now.elapsed_since(t0).as_secs_f32());
                let long = !near(target, rect, cell);
                let inside = self
                    .last_lit
                    .is_some_and(|lit| now.elapsed_since(lit) < REIGNITE);
                if long && inside && variant() == 1 {
                    self.moving = None;
                    self.fire = None;
                    return;
                }
                if long && inside && variant() == 2 {
                    self.turn(centre(from), rect);
                    self.moving = None;
                    return;
                }
                if long && variant() == 3 && !self.sweep_ok(now) {
                    self.moving = None;
                    self.fire = None;
                    return;
                }
                self.turn(centre(from), rect);
                let next = if !long {
                    Segment::quick(from, rect)
                } else {
                    self.ignite(centre(from), rect, now, cell);
                    self.note_sweep(now);
                    self.long(from, rect)
                };
                self.moving = Some((next, now));
            }
            None => {
                if variant() == 3 && !self.sweep_ok(since) {
                    self.fire = None;
                    return;
                }
                let from = target.corners();
                self.turn(centre(from), rect);
                self.ignite(centre(from), rect, since, cell);
                self.note_sweep(since);
                self.moving = Some((self.long(from, rect), since));
            }
        }
    }

    /// Scratch, variant 3: whether a fourth sweep would fall inside one
    /// second of the third-last.
    fn sweep_ok(&self, at: M) -> bool {
        match self.sweeps[0] {
            Some(oldest) if self.sweeps.iter().all(|s| s.is_some()) => {
                at.elapsed_since(oldest) >= Duration::from_secs(1)
            }
            _ => true,
        }
    }

    fn note_sweep(&mut self, at: M) {
        self.sweeps = [self.sweeps[1], self.sweeps[2], Some(at)];
    }

    /// The axis of a segment from `from` to the centre of `rect`, unless the
    /// two are the same point, which has none.
    fn turn(&mut self, from: Pt, rect: Rect) {
        let to = centre(rect.corners());
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let len = dx.hypot(dy);
        if len > 1e-3 {
            self.axis = Pt {
                x: dx / len,
                y: dy / len,
            };
        }
    }

    /// A long move's constants: kitty's per corner for a quad and an effect,
    /// sfte's one for a sweep.
    fn long(&self, from: baud_render::Corners, to: Rect) -> Segment {
        match self.style {
            Style::Smear(Shape::Quad) | Style::Effect(_) => Segment::new(from, to),
            Style::Smear(Shape::Sweep) => Segment::sweep(from, to),
        }
    }

    /// An effect's long move from `tail` to `rect`, starting `at`: a new
    /// ignition, unless the last was lit within [`REIGNITE`] and still burns,
    /// which the quad carries on retargeted.
    fn ignite(&mut self, tail: Pt, rect: Rect, at: M, cell: (f32, f32)) {
        let Style::Effect(effect) = self.style else {
            return;
        };
        if self
            .last_lit
            .is_some_and(|lit| at.elapsed_since(lit) < reignite())
        {
            return;
        }
        self.last_lit = Some(at);
        let ignition = Ignition::ignite(tail, rect, self.next_seed, cell.1);
        self.next_seed = Seed(self.next_seed.0.wrapping_add(1));
        self.fire = Some(Burning {
            at,
            ignition,
            end: effect.end(&ignition),
        });
    }

    /// **Output scrolled the screen `dy` pixels, y down**, at `now` (the
    /// display link's clock, which [`Chase::frame`] reads): what the trail
    /// drew from the text rides with it. A move in flight is rebased where
    /// its corners are, moved by `dy` and on its own constants, while it has
    /// ridden at most [`THRESHOLD`] rows; past that it folds in on the fast
    /// constant and rides no more; and once every corner is above the row
    /// that leaves the window, `cell_h` pixels up, it is dropped. A burning
    /// effect's path rides whole. Nothing moves below `floor`, one window
    /// above the top. The rest state -- where the cursor was taken, a move
    /// waiting out its dwell -- never moves, so the moves that trail are the
    /// ones they were.
    pub fn ride(&mut self, dy: f32, now: M, floor: f32, cell_h: f32) {
        if let (Some((seg, t0)), Carry::Riding(r)) = (self.moving, self.carry) {
            let at = seg.at(now.elapsed_since(t0).as_secs_f32());
            let from = baud_render::Corners(at.0.map(|p| Pt {
                x: p.x,
                y: (p.y + dy).max(floor),
            }));
            if from.0.iter().all(|p| p.y <= -cell_h - 0.5) {
                // Above the leaving row by half a pixel, the reach of its
                // edge: nothing of it shows.
                self.moving = None;
            } else if (r + dy).abs() > THRESHOLD * cell_h {
                // Past the bound: this frame is rigid with the text, and the
                // move folds in from here, as a short move in flight does,
                // and rides no more.
                self.moving = Some((Segment::quick(from, seg.to), now));
                self.carry = Carry::Folded;
            } else {
                self.moving = Some((Segment { from, ..seg }, now));
                self.carry = Carry::Riding(r + dy);
            }
        }
        if let Some(b) = self.fire.as_mut() {
            let applied = (b.ignition.tail.y + dy).max(floor) - b.ignition.tail.y;
            b.ignition = b.ignition.translated(applied);
        }
    }

    /// Whether the trail, if any, has folded to within [`THRESHOLD`] cells of
    /// `rect` at every corner: at rest it has nothing to fold.
    fn folded(&self, now: M, rect: Rect, cell: (f32, f32)) -> bool {
        let Some((seg, t0)) = self.moving else {
            return true;
        };
        let at = seg.at(now.elapsed_since(t0).as_secs_f32()).0;
        at.iter().zip(rect.corners().0).all(|(p, q)| {
            let cells = |a: f32, b: f32, size: f32| ((b - a) / size.max(1.0)).abs();
            cells(p.x, q.x, cell.0) <= THRESHOLD && cells(p.y, q.y, cell.1) <= THRESHOLD
        })
    }

    /// Whether the tick owes a frame at `now`: every frame while a trail is
    /// in flight or a fire burns, and the one a move's dwell runs out on --
    /// and the one a fire goes out on, which draws it gone.
    pub fn owes(&self, now: M) -> Owes {
        let due = self
            .pending
            .is_some_and(|(_, since)| now.elapsed_since(since) >= DWELL);
        if self.moving.is_some() || self.fire.is_some() || due {
            Owes::Frame
        } else {
            Owes::Nothing
        }
    }

    /// Forget where the cursor was, and any trail: the next sighting starts
    /// none. A resize moves every cell under it and a screen switch shows
    /// another cursor, and a trail across either would join two screens.
    pub fn snap(&mut self) {
        *self = Chase {
            next_seed: self.next_seed,
            last_lit: self.last_lit,
            ..Chase::new(self.style)
        };
    }

    /// Draw another trail from now on: a snap into `style`. **Not
    /// `Chase::new`**, which forgets when the last ignition was lit, so a
    /// switch back could light a fourth within a second against
    /// [`REIGNITE`]; and not the style alone, which would leave a move or a
    /// fire in flight that the new trail never draws -- and the tick owes a
    /// frame for it on every tick, for ever.
    pub fn restyle(&mut self, style: Style) {
        *self = Chase {
            next_seed: self.next_seed,
            last_lit: self.last_lit,
            ..Chase::new(style)
        };
    }
}

/// The centre of the corners' bounds: where a fire's path starts from.
fn centre(c: baud_render::Corners) -> Pt {
    let xs = c.0.map(|p| p.x);
    let ys = c.0.map(|p| p.y);
    let mid = |v: [f32; 4]| {
        let lo = v.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        (lo + hi) * 0.5
    };
    Pt {
        x: mid(xs),
        y: mid(ys),
    }
}

/// Whether a move from `target` to `rect` is at most [`THRESHOLD`] cells on
/// each axis, kitty's `start_threshold` test
/// (`[[source: kitty-cursor-trail]]` 66-70).
fn near(target: Rect, rect: Rect, cell: (f32, f32)) -> bool {
    let cells = |a: f32, b: f32, size: f32| ((b - a) / size.max(1.0)).round().abs();
    cells(target.left, rect.left, cell.0) <= THRESHOLD
        && cells(target.top, rect.top, cell.1) <= THRESHOLD
}

