//! The quad, sweep and effect trails' driver: when a cursor move starts one,
//! when it retargets, and which frames it owes; and the ghost's rules, when
//! it sweeps and how far ([`ghost_sweep`], [`ghost_progress`]).
//!
//! **The geometry lives in `baud_render::trail`**: kitty's four corners
//! chasing the cursor's, each with its own time constant, or sfte's tail --
//! the cursor's rectangle, every corner on one constant -- in closed form.
//! What lives here is which moves become a trail, the same for both, after
//! two sources:
//!
//! * **A move of two cells or fewer, from rest, snaps.** kitty's
//!   `cursor_trail_start_threshold 2`
//!   (`[[source: kitty-cursor-trail-v0492]]` 66-70, the option at
//!   `[[source: kitty-options-definition-v0492]]` 483-485): typing moves
//!   the cursor a cell at a time, and a trail per keystroke is noise. A move
//!   while a trail is in flight retargets it whatever its size, as kitty's
//!   does.
//! * **The cursor has to rest at its new place before the trail follows
//!   it** -- kitty's rule: its target moves only once `cursor_trail` ms
//!   have passed since the program last moved the cursor
//!   (`[[source: kitty-cursor-trail-v0492]]` 174). The number is sfte's,
//!   10 ms by its own "sensible default" (`[[source: sfte]]` 13891); sfte's own rule
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
//! * **At most three ignitions a second.** A long move within [`REIGNITE`] of
//!   the last ignition retargets the quad and keeps the flame that is already
//!   burning, rather than lighting a new one. It limits ignitions, not
//!   flashes: counted on the frames the trails draw, one pixel flashed up to
//!   23 times a second under it, and at 32 pt the fire and the orb fail WCAG
//!   2.3.1 on a light ground; at the default 13 pt every effect passes, by
//!   the area clause alone (note AL).
//! * **Typing never ignites**: a move of two cells or fewer snaps, as it does
//!   for the quad, and a burning flame follows the cursor it snapped to.
//!
//! **Above a 32 pt cell, a repeat pass is drawn under the flash thresholds**
//! (note AL, option O2; every trail takes the bound, the ghost's sweeps
//! weighed as a long move is, on the one [`Ledger`] every trail shares,
//! so a change of trail keeps the second's passes). The
//! first pass over a stretch of screen in any second is drawn as it is; a
//! long move whose band meets a full pass's within [`LEDGER`] is marked
//! [`Strength::UnderThreshold`], and the renderer caps it against the
//! colours the cells under it draw, so no pixel's change there is a WCAG 2.2
//! general or red transition. A pass over an image placed under the text
//! draws nothing. A selection and the cursor are grounds by the colours
//! they draw, and a dimmed page is read over a white and a black desktop.
//!
//! A flame whose quad has landed burns where the chase rests: through a long
//! move's dwell that is the cell the cursor left, which is where the move,
//! once taken, starts from. Drawn at the cursor, it jumped ahead for the dwell
//! and back when the move was taken -- on every line of a wheel scroll that
//! carries a placed cursor three rows, as helix's does.
//!
//! The clock is the driver's -- the display link's timestamp -- and a move
//! waiting out its dwell owes the tick one frame when the dwell runs out
//! ([`Chase::owes`]), because after a lone keystroke's echo nothing else
//! would draw one. The trail is backdated to when the cursor was first seen at
//! its new place, so the frame that starts it is already that far along.
//!
//! **A program that hides the cursor fades the trail out** rather than
//! ending it, as kitty does (`[[source: kitty-cursor-trail]]` 64, 134-139):
//! over [`FADE`] from the last frame the cursor was shown on, the trail
//! going on as it would, and back at that rate when it is shown again
//! ([`Chase::frame`]). sfte's and Neovide's end at once (note W, 5.12).

use baud_render::det::Det;
use core::time::Duration;

use baud_domain::{Chan, Col, Row};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{Points, Pt, Rect, Segment, Shape, Smear, Strength, TrailLook};

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

/// **How long a hidden cursor's trail takes to fade out**, seconds, and a
/// shown one's to come back: kitty's slow corner's decay time, which its
/// trail's opacity falls by while the cursor is hidden and rises by while
/// it is shown (`[[source: kitty-cursor-trail]]` 134-139).
pub const FADE: f32 = baud_render::TAU_SLOW;

/// **The size gate of the flash bound** (note AL, option O2): on a cell
/// taller than this, a long move over a stretch of screen a full pass swept
/// less than [`LEDGER`] before is a repeat, drawn under the flash
/// thresholds ([`Strength::UnderThreshold`]). At 32 pt and up the fire and
/// the orb fail WCAG 2.3.1 as shipped (note AL); JetBrains Mono's cell is
/// 32 pt at 24 pt, which the strict test keeps below it.
pub const GATE: Points = Points(32.0);

/// How long a full pass holds the screen it swept: WCAG 2.3.1's "any one
/// second period".
pub const LEDGER: Duration = Duration::from_secs(1);

/// **How fast the fire plays against the display's clock**: its flame, its
/// embers and the quad it rises off all run 1.6 times as fast, so a long
/// move's fire is out 0.28 to 0.53 s after it lights rather than 0.45 to
/// 0.85 s -- `fire::LIFE` and its last ember's death are seconds of its own
/// clock, read over this rate. What guards against flashing keeps the
/// display's clock: the three ignitions a second ([`REIGNITE`]), the
/// [`LEDGER`]'s second, the dwell and a hidden cursor's fade.
pub const FIRE_RATE: f32 = 1.6;

/// **How long the ghost takes to sweep** from the cell the cursor left to
/// the cell it reached, seconds: short and snappy, and paid only while the
/// ghost is the trail.
pub const GHOST_S: f64 = 0.1;

/// A ghost's sweep as it starts: the cell it left, the cell it reached, and
/// how it is drawn.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GhostSweep {
    pub from: (Row, Col),
    pub to: (Row, Col),
    pub strength: Strength,
}

/// **The ghost's rule for a sweep**: on any move of the cursor's cell from
/// `last` to `now`, from the cell it was last seen at -- typing included,
/// which the quad's threshold snaps -- and a sweep in flight restarts from
/// there; `None` while it rests, and until it is first seen. **Each sweep
/// is weighed on `ledger`** at `at`, over the band from the rectangle of the
/// cell it left to the one it reached ([`ghost_band`], `rect` giving each
/// cell's), on cells `cell` pixels and `height` points tall, as a long move
/// is.
pub fn ghost_sweep<M: Moment>(
    ledger: &mut Ledger<M>,
    last: Option<(Row, Col)>,
    now: Option<(Row, Col)>,
    at: M,
    rect: impl Fn((Row, Col)) -> Rect,
    (cell, height): ((f32, f32), Points),
) -> Option<GhostSweep> {
    let (from, to) = match (last, now) {
        (Some(from), Some(to)) if from != to => (from, to),
        _ => return None,
    };
    let band = ghost_band(rect(from), rect(to), cell);
    Some(GhostSweep {
        from,
        to,
        strength: ledger.weigh(band, at, height),
    })
}

/// How far a ghost started at `start` has swept at `now`, both seconds on
/// the display link's clock: 1.0 and past once it has landed.
pub fn ghost_progress(start: f64, now: f64) -> f32 {
    ((now - start) / GHOST_S) as f32
}

/// The full passes the ledger holds. A long move that finds every one of
/// them still inside [`LEDGER`] is drawn as a repeat rather than forget one.
const SWEPT: usize = 8;

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
/// fast constant and rides no more -- so a trail in flight when output
/// starts streaming lands rather than hanging a row or more above the
/// cursor for as long as the stream lasts.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Carry {
    Riding(f32),
    #[cfg_attr(
        all(target_os = "linux", not(test)),
        expect(dead_code, reason = "the Linux driver draws no smooth scroll to ride")
    )]
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
    /// **The ledger** of full passes, which a snap and a restyle keep, as
    /// they keep [`Chase::last_lit`].
    swept: Ledger<M>,
    /// How the move in flight is drawn, until the next long move.
    strength: Strength,
    /// **The trail's opacity** while it is not whole, or is not rising to
    /// whole with the cursor shown: [`Opacity`].
    opacity: Option<Opacity<M>>,
    /// When the last frame was, which a cursor shown or hidden since counts
    /// its change of opacity from, as kitty's step does.
    last: Option<M>,
    /// The cursor as the last frame that followed one drew it: the hole a
    /// hidden cursor's trail keeps as it fades.
    seen: Option<Rect>,
}

/// Whether a frame has a cursor to follow: shown, or placed where a program
/// hid it -- or hidden, with nothing placed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sight {
    Shown,
    Hidden,
}

/// **kitty's opacity** (`[[source: kitty-cursor-trail]]` 129-141): a
/// level, the moment it was taken -- the last frame before the cursor was
/// last shown or hidden -- and which way it goes from there: down to
/// nothing over [`FADE`] while the cursor is hidden, back up at the same
/// rate while it is shown. kitty adds a step a frame, the seconds since the
/// last over the decay time; this is their sum in closed form, so a frame
/// is a function of the time alone and not of how many came between.
#[derive(Clone, Copy)]
struct Opacity<M> {
    level: f32,
    since: M,
    toward: Sight,
}

impl<M: Moment> Opacity<M> {
    /// The level at `now`, 0 to 1.
    fn at(self, now: M) -> f32 {
        let step = now.elapsed_since(self.since).as_secs_f32() / FADE;
        match self.toward {
            Sight::Shown => (self.level + step).min(1.0),
            Sight::Hidden => (self.level - step).max(0.0),
        }
    }
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
            swept: Ledger::new(),
            strength: Strength::Full,
            opacity: None,
            last: None,
            seen: None,
        }
    }

    /// **A frame's trail**: the cursor as this frame draws it, or where a
    /// program placed the one it hid (`Term::placed_cursor`) -- `None` when it
    /// is hidden unplaced or the window is looking back -- at `now`, on cells
    /// `cell` pixels wide and tall and `height` points tall, and what the
    /// frame draws.
    ///
    /// **A hidden cursor's trail fades out**, as kitty's does
    /// (`[[source: kitty-cursor-trail]]` 64, 134-139): a trail drawing when
    /// the cursor is hidden goes on -- its corners chasing where the cursor
    /// was taken, an effect ageing, the hole where the cursor was last
    /// drawn -- at an opacity falling from the last frame that showed the
    /// cursor to nothing over [`FADE`], and is out at the fade's end or its
    /// own, whichever is first. Shown again, it comes back at the same rate.
    /// Looking back snaps the chase first (`chase_frame`), so a trail there
    /// is gone at once.
    pub fn frame(
        &mut self,
        cursor: Option<Rect>,
        now: M,
        cell: (f32, f32),
        height: Points,
    ) -> Option<TrailLook> {
        let level = match cursor {
            Some(_) => self.see(Sight::Shown, now),
            None => self.see(Sight::Hidden, now),
        };
        let Some(rect) = cursor else {
            // Nothing new to arrive at: a move waiting out its dwell is
            // dropped, and the place the cursor was taken stays where a
            // trail would start from. What is drawing fades where it was
            // going, until the fade is out (kitty's 64).
            self.pending = None;
            return match self.seen {
                Some(hole) if level > 0.0 => self.look(hole, now, level),
                _ => {
                    self.moving = None;
                    self.fire = None;
                    None
                }
            };
        };
        self.seen = Some(rect);
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
                        self.take(target, rect, since, now, (cell, height));
                    }
                }
                _ => self.pending = Some((rect, now)),
            }
        }
        self.look(rect, now, level)
    }

    /// **The trail's opacity at `now`**, for a frame whose cursor is
    /// `sight`: a change of sight counts from the last frame, as kitty's
    /// step does, and a trail whole with the cursor shown keeps nothing.
    fn see(&mut self, sight: Sight, now: M) -> f32 {
        let from = self.last.unwrap_or(now);
        self.last = Some(now);
        let opacity = match self.opacity {
            Some(o) if o.toward == sight => o,
            Some(o) => Opacity {
                level: o.at(from),
                since: from,
                toward: sight,
            },
            None => match sight {
                Sight::Shown => return 1.0,
                Sight::Hidden => Opacity {
                    level: 1.0,
                    since: from,
                    toward: sight,
                },
            },
        };
        let level = opacity.at(now);
        self.opacity = match (sight, level >= 1.0) {
            (Sight::Shown, true) => None,
            _ => Some(opacity),
        };
        level
    }

    /// **The seconds of the trail's own clock from `since` to `now`**: the
    /// display's, but for the fire's, which runs [`FIRE_RATE`] times as
    /// fast -- every read of a move's time and of an ignition's age takes
    /// it, so a retarget, a fold and a ride start from the corners the
    /// frame drew.
    fn clock(&self, since: M, now: M) -> f32 {
        let secs = now.elapsed_since(since).as_secs_f32();
        match self.style {
            Style::Effect(Effect::Fire) => secs * FIRE_RATE,
            Style::Smear(_) | Style::Effect(_) => secs,
        }
    }

    /// **What the frame draws at `now`**, around `hole`, the cursor's
    /// rectangle as this frame or, hidden, the last that drew it, at the
    /// opacity `level`.
    fn look(&mut self, hole: Rect, now: M, level: f32) -> Option<TrailLook> {
        let opacity = Chan::from_unit(level);
        // The corners this frame, while a move is in flight.
        let corners = self.moving.and_then(|(seg, t0)| {
            let t = self.clock(t0, now);
            (t < seg.lasts()).then(|| seg.at(t))
        });
        if corners.is_none() {
            self.moving = None;
        }
        // The hole is the cursor as this frame draws it, which a move still
        // inside its dwell has already left the segment's target for: the
        // trail never covers the cursor. Hidden, it is where the cursor was.
        match self.style {
            Style::Smear(shape) => corners.map(|corners| {
                TrailLook::Smear(Smear {
                    corners,
                    hole,
                    shape,
                    strength: self.strength,
                    opacity,
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
                let age = self.clock(ig.at, now);
                if age >= ig.end {
                    // Out: the flame and every ember, and the quad it rode
                    // with them.
                    self.fire = None;
                    self.moving = None;
                    return None;
                }
                // Once the corners land the flame burns on where the chase
                // rests: the cursor's cell after a snap or a take, and the
                // cell the cursor left while a long move waits out its dwell
                // -- where `take` starts the next segment from. Drawn at the
                // cursor, it jumped to the new cell for the dwell and back
                // when the move was taken.
                Some(TrailLook::Vfx(Vfx {
                    effect,
                    emitter: Smear {
                        corners: corners.unwrap_or_else(|| self.target.unwrap_or(hole).corners()),
                        hole,
                        shape: Shape::Quad,
                        strength: self.strength,
                        opacity,
                    },
                    ignition: ig.ignition.at(age),
                    axis: self.axis,
                }))
            }
        }
    }

    /// The cursor has rested at `rect` long enough: follow it.
    fn take(
        &mut self,
        target: Rect,
        rect: Rect,
        since: M,
        now: M,
        (cell, height): ((f32, f32), Points),
    ) {
        self.pending = None;
        self.target = Some(rect);
        self.carry = Carry::Riding(0.0);
        match self.moving {
            // In flight: retarget from where the corners are now, so they
            // do not jump -- and a short move on the fast constant, so typing
            // after a jump folds the trail in rather than keeping it alive.
            Some((seg, t0)) => {
                let from = seg.at(self.clock(t0, now));
                self.turn(centre(from), rect);
                let next = if near(target, rect, cell) {
                    Segment::quick(from, rect)
                } else {
                    self.ignite(centre(from), rect, now, cell);
                    self.weigh(from, rect, now, (cell, height));
                    self.long(from, rect)
                };
                self.moving = Some((next, now));
            }
            None => {
                let from = target.corners();
                self.turn(centre(from), rect);
                self.ignite(centre(from), rect, since, cell);
                self.weigh(from, rect, since, (cell, height));
                self.moving = Some((self.long(from, rect), since));
            }
        }
    }

    /// **How a long move starting `at` from `from` to `rect` is drawn.** Its
    /// band is the box of its start corners and its target, out by the
    /// effect's reach in units -- the cell's height -- or by a cell for a
    /// smear. On a cell taller than [`GATE`], a band that meets the band of
    /// a full pass started less than [`LEDGER`] before is a repeat over that
    /// screen; any other is full, and takes the ledger's slot of a pass
    /// that has expired. A move that draws nothing -- an effect's, refused
    /// by the three-a-second guard with nothing burning -- is not weighed.
    fn weigh(
        &mut self,
        from: baud_render::Corners,
        rect: Rect,
        at: M,
        (cell, height): ((f32, f32), Points),
    ) {
        let out = match self.style {
            Style::Effect(_) if self.fire.is_none() => return,
            Style::Effect(e) => (e.reach() * cell.1, e.reach() * cell.1),
            Style::Smear(_) => cell,
        };
        let band = band(from.0.into_iter().chain(rect.corners().0), out);
        self.strength = self.swept.weigh(band, at, height);
    }

    /// The axis of a segment from `from` to the centre of `rect`, unless the
    /// two are the same point, which has none.
    fn turn(&mut self, from: Pt, rect: Rect) {
        let to = centre(rect.corners());
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let len = dx.det_hypot(dy);
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
            .is_some_and(|lit| at.elapsed_since(lit) < REIGNITE)
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
    #[cfg(any(target_os = "macos", test))]
    pub fn ride(&mut self, dy: f32, now: M, floor: f32, cell_h: f32) {
        if let (Some((seg, t0)), Carry::Riding(r)) = (self.moving, self.carry) {
            let at = seg.at(self.clock(t0, now));
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
        let at = seg.at(self.clock(t0, now)).0;
        at.iter().zip(rect.corners().0).all(|(p, q)| {
            let cells = |a: f32, b: f32, size: f32| ((b - a) / size.max(1.0)).abs();
            cells(p.x, q.x, cell.0) <= THRESHOLD && cells(p.y, q.y, cell.1) <= THRESHOLD
        })
    }

    /// Whether the tick owes a frame at `now`: every frame while a trail is
    /// in flight or a fire burns, a hidden cursor's fading one included, and
    /// the one a move's dwell runs out on -- and the one a fire or a fade
    /// goes out on, which draws it gone.
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

    /// **When the next frame this trail owes is due, if it is not owed
    /// already**: the end of a move's dwell. While a trail is in flight or
    /// a fire burns every frame is owed ([`Chase::owes`]), and the Linux
    /// loop paces those on the compositor's frame events rather than on a
    /// deadline, so this names none for them.
    #[cfg(any(target_os = "linux", test))]
    pub fn deadline(&self) -> Option<M> {
        match (self.moving, self.fire, self.pending) {
            (None, None, Some((_, since))) => Some(since.after(DWELL)),
            _ => None,
        }
    }

    /// Forget where the cursor was, and any trail: the next sighting starts
    /// none. A resize moves every cell under it and a screen switch shows
    /// another cursor, and a trail across either would join two screens.
    pub fn snap(&mut self) {
        *self = Chase {
            next_seed: self.next_seed,
            last_lit: self.last_lit,
            swept: self.swept,
            ..Chase::new(self.style)
        };
    }

    /// **The ledger** this chase weighs its passes on, which the ghost's
    /// sweeps are weighed on too, so a change of trail keeps the second's.
    pub fn ledger(&mut self) -> &mut Ledger<M> {
        &mut self.swept
    }

    /// Draw another trail from now on: a snap into `style`. **Not
    /// `Chase::new`**, which forgets when the last ignition was lit, so a
    /// switch back could light a fourth within a second against
    /// [`REIGNITE`]; and not the style alone, which would leave a move or a
    /// fire in flight that the new trail never draws -- and the tick owes a
    /// frame for it on every tick, for ever.
    #[cfg(any(target_os = "macos", target_os = "linux", test))]
    pub fn restyle(&mut self, style: Style) {
        *self = Chase {
            next_seed: self.next_seed,
            last_lit: self.last_lit,
            swept: self.swept,
            ..Chase::new(style)
        };
    }
}

/// **The ledger**: the last [`SWEPT`] full passes, when each started and the
/// band it can draw on, which every trail weighs its passes against.
#[derive(Clone, Copy)]
pub struct Ledger<M> {
    swept: [Option<(M, Rect)>; SWEPT],
}

impl<M: Moment> Ledger<M> {
    pub fn new() -> Ledger<M> {
        Ledger {
            swept: [None; SWEPT],
        }
    }

    /// **How a pass over `band` starting `at` is drawn**, on a cell
    /// `height` points tall. Above [`GATE`], a band that meets the band of
    /// a full pass started less than [`LEDGER`] before is a repeat over that
    /// screen, and so is any pass while every slot holds a live one, rather
    /// than forget it; any other is full, and takes the slot of a pass that
    /// has expired. Below the gate every pass is full.
    pub fn weigh(&mut self, band: Rect, at: M, height: Points) -> Strength {
        let live = |t: M| at.elapsed_since(t) < LEDGER;
        let meets = |b: Rect| {
            b.left <= band.right
                && band.left <= b.right
                && b.top <= band.bottom
                && band.top <= b.bottom
        };
        let gated = height > GATE;
        let repeat = self
            .swept
            .iter()
            .flatten()
            .any(|&(t, b)| live(t) && meets(b));
        let free = self
            .swept
            .iter()
            .position(|e| e.is_none_or(|(t, _)| !live(t)));
        match (gated && repeat, free) {
            (false, Some(slot)) => {
                self.swept[slot] = Some((at, band));
                Strength::Full
            }
            // Below the gate nothing is a repeat, and a full ledger there
            // has nothing to hold: every pass is full.
            (false, None) if !gated => Strength::Full,
            _ => Strength::UnderThreshold,
        }
    }
}

impl<M: Moment> Default for Ledger<M> {
    fn default() -> Ledger<M> {
        Ledger::new()
    }
}

/// The box of `points`, out by `out` on each axis: a pass's band.
fn band(points: impl Iterator<Item = Pt>, out: (f32, f32)) -> Rect {
    points.fold(
        Rect {
            left: f32::INFINITY,
            top: f32::INFINITY,
            right: f32::NEG_INFINITY,
            bottom: f32::NEG_INFINITY,
        },
        |b, p| Rect {
            left: b.left.min(p.x - out.0),
            top: b.top.min(p.y - out.1),
            right: b.right.max(p.x + out.0),
            bottom: b.bottom.max(p.y + out.1),
        },
    )
}

/// **The band a ghost's sweep can draw on**, from the rectangle of the
/// cell it left to the one it reached: their box, out by a cell, as a
/// smear's long move is weighed.
pub fn ghost_band(from: Rect, to: Rect, cell: (f32, f32)) -> Rect {
    band(from.corners().0.into_iter().chain(to.corners().0), cell)
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
/// (`[[source: kitty-cursor-trail-v0492]]` 66-70).
fn near(target: Rect, rect: Rect, cell: (f32, f32)) -> bool {
    let cells = |a: f32, b: f32, size: f32| ((b - a) / size.max(1.0)).round().abs();
    cells(target.left, rect.left, cell.0) <= THRESHOLD
        && cells(target.top, rect.top, cell.1) <= THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_domain::Rows;
    use baud_font::Px;
    use baud_render::{Corners, Slide};
    use std::time::Instant;

    const CELL: (f32, f32) = (17.0, 34.0);

    /// **The chase calls no platform math**: the axis it hands the plasma
    /// is a pixel input on every OS, so its length is `det`'s, not the
    /// host's libm's. Perturbation: std's hypot back in `turn` fails it.
    #[test]
    fn the_chase_calls_no_platform_math() {
        let src = include_str!("trail.rs");
        for call in baud_render::det::PLATFORM {
            assert!(!src.contains(call), "trail.rs calls the platform's {call}");
        }
        let control = concat!(".det_", "hypot(");
        assert!(
            src.contains(control),
            "the control: the scan reads the chase"
        );
    }

    /// The cell's height in points: `CELL` is 13 pt at 2x, below the gate.
    const PT: Points = Points(17.0);

    /// The ghost lands `GHOST_S` after it starts, on the display link's
    /// clock, and is half way at half that.
    #[test]
    fn the_ghost_lands_after_its_sweep_time() {
        assert_eq!(ghost_progress(2.0, 2.0), 0.0);
        assert_eq!(ghost_progress(2.0, 2.0 + GHOST_S / 2.0), 0.5);
        assert!(ghost_progress(2.0, 2.0 + GHOST_S) >= 1.0);
    }

    impl Chase<Instant> {
        /// The frame's trail as an effect, for the fire's tests.
        fn fire(&mut self, cursor: Option<Rect>, now: Instant) -> Option<Vfx> {
            match self.frame(cursor, now, CELL, PT) {
                Some(TrailLook::Vfx(f)) => Some(f),
                None => None,
                Some(other) => panic!("a fire chase drew {other:?}"),
            }
        }
    }

    /// **A move waiting out its dwell names the dwell's end; a trail in
    /// flight, at rest or never seen names none**, so the Linux wait wakes
    /// for the one frame no event would bring and paces the rest on the
    /// compositor. Perturbation: name the dwell's start -- the second
    /// assertion fails.
    #[test]
    fn only_a_dwell_names_a_deadline() {
        let t0 = Instant::now();
        let (from, to) = (at(10.0, 5.0), at(60.0, 5.0));
        let mut q: Chase<Instant> = Chase::new(Style::Smear(Shape::Quad));
        assert_eq!(q.deadline(), None, "never seen");
        q.frame(Some(from), t0, CELL, PT);
        assert_eq!(q.deadline(), None, "at rest");
        q.frame(Some(to), t0 + MS * 3, CELL, PT);
        assert_eq!(q.deadline(), Some(t0 + MS * 3 + DWELL), "the dwell");
        assert!(q.frame(Some(to), t0 + MS * 20, CELL, PT).is_some(), "taken");
        assert_eq!(q.deadline(), None, "in flight: every frame is owed");
        assert_eq!(q.owes(t0 + MS * 21), Owes::Frame);
    }

    /// **A long move lights a fire that burns past its chase** until its last
    /// ember dies: the quad lands, the flame burns on at the cursor, the tick
    /// owes every frame until the end and the one that draws it out, and
    /// nothing after -- each on the fire's clock, [`FIRE_RATE`] times the
    /// display's.
    #[test]
    fn a_fire_burns_past_its_chase_until_its_last_ember() {
        let t0 = Instant::now();
        let (from, to) = (at(10.0, 5.0), at(60.0, 5.0));
        let mut q = Chase::new(Style::Effect(Effect::Fire));
        assert_eq!(q.fire(Some(from), t0), None, "first sighting");
        assert_eq!(q.fire(Some(to), t0 + MS * 8), None, "the dwell");
        let f = q
            .fire(Some(to), t0 + MS * 20)
            .expect("lit once the dwell is out");
        assert!(
            (f.ignition.age - 0.012 * FIRE_RATE).abs() < 0.001,
            "backdated: {}",
            f.ignition.age
        );
        assert_eq!(f.ignition.seed, Seed(1));
        assert_eq!(
            (f.ignition.dir.x, f.ignition.dir.y),
            (1.0, 0.0),
            "along the move"
        );
        assert_eq!(f.emitter.hole, to, "the cursor is bare");
        let lasts = Segment::new(from.corners(), to).lasts();
        let end = Effect::Fire.end(&Ignition::ignite(
            centre(from.corners()),
            to,
            Seed(1),
            CELL.1,
        ));
        assert!(
            end > lasts + 0.05,
            "the fire outlives the chase: {end} against {lasts}"
        );
        let ms = |secs: f32| MS * u32::try_from((secs / FIRE_RATE * 1000.0) as i64).unwrap();
        let landed = q
            .fire(Some(to), t0 + MS * 8 + ms(lasts + 0.02))
            .expect("still burning after the quad lands");
        assert_eq!(
            landed.emitter.corners,
            to.corners(),
            "the flame burns on at the cursor"
        );
        let last = t0 + MS * 8 + ms(end - 0.005);
        assert!(
            q.fire(Some(to), last).is_some(),
            "burning just before its end"
        );
        assert_eq!(q.owes(last), Owes::Frame);
        let out = t0 + MS * 8 + ms(end + 0.005);
        assert_eq!(q.owes(out), Owes::Frame, "the frame that draws it out");
        assert_eq!(q.fire(Some(to), out), None, "out");
        assert_eq!(q.owes(out + MS * 8), Owes::Nothing, "and nothing after");
    }

    /// **The fire plays 1.6 times as fast, and so does the quad it rises
    /// off**, on the clips' diagonal: the fifth move of
    /// `scripts/trail-demo.sh`, from the top-left mark to the bottom-right.
    /// Its flame and embers are drawn at 1.6 times their age's seconds, so
    /// the fire burns until its end over 1.6 and is out after it -- by
    /// 0.536 s, where on the display's clock it burned past 0.8 s. The
    /// quad's corners are the segment's at 1.6 times the seconds since the
    /// move started. The rate is written here rather than read from the
    /// constant, so a change of it fails. Perturbations: the rate at 1.0
    /// fails it at the age; the age scaled with the frame's corners left on
    /// the display's clock fails it at the corners.
    #[test]
    fn the_fire_and_its_quad_play_at_one_point_six() {
        const RATE: f32 = 1.6;
        let t0 = Instant::now();
        let (from, to) = (at(7.0, 5.0), at(71.0, 19.0));
        let mut q = Chase::new(Style::Effect(Effect::Fire));
        assert_eq!(q.fire(Some(from), t0), None, "first sighting");
        assert_eq!(q.fire(Some(to), t0 + MS * 8), None, "the dwell");
        let lit = q.fire(Some(to), t0 + MS * 20).expect("lit");
        let since = t0 + MS * 8;
        assert!(
            (lit.ignition.age - 0.012 * RATE).abs() < 1e-4,
            "backdated, at the fire's rate: {}",
            lit.ignition.age
        );
        let ig = Ignition::ignite(centre(from.corners()), to, Seed(1), CELL.1);
        assert_eq!(lit.ignition.seed, Seed(1));
        let end = Effect::Fire.end(&ig);
        assert!(
            end > 0.8,
            "the control: on the display's clock this fire burned to {end} s"
        );
        let seg = Segment::new(from.corners(), to);
        for ms in [30u32, 60, 120] {
            let f = q.fire(Some(to), since + MS * ms).expect("burning");
            let want = seg.at(ms as f32 / 1000.0 * RATE);
            let moved = f
                .emitter
                .corners
                .0
                .iter()
                .zip(want.0)
                .all(|(p, w)| (p.x - w.x).abs() < 1e-2 && (p.y - w.y).abs() < 1e-2);
            assert!(
                moved,
                "{ms} ms in, the quad is the segment's at {RATE}x: {:?} against {want:?}",
                f.emitter.corners
            );
        }
        let ms = |secs: f32| MS * u32::try_from((secs * 1000.0) as i64).unwrap();
        assert!(
            q.fire(Some(to), since + ms(end / RATE - 0.005)).is_some(),
            "burning just before its end over the rate"
        );
        let out = since + ms(end / RATE + 0.005);
        assert!(out <= since + MS * 536, "out by 0.536 s: {:?}", out - since);
        assert_eq!(q.fire(Some(to), out), None, "out just after it");
        assert_eq!(q.owes(out + MS * 8), Owes::Nothing, "and nothing after");
    }

    /// **A fire lights at most three times a second**: a long move within
    /// [`REIGNITE`] of the last ignition retargets the quad and keeps the
    /// flame that is burning -- its seed and its age -- and one after it
    /// lights a new one.
    #[test]
    fn a_fire_lights_at_most_three_times_a_second() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Effect(Effect::Fire));
        q.fire(Some(at(10.0, 5.0)), t0);
        q.fire(Some(at(60.0, 5.0)), t0 + MS * 8);
        let first = q.fire(Some(at(60.0, 5.0)), t0 + MS * 20).expect("lit");
        assert_eq!(first.ignition.seed, Seed(1));
        // 212 ms after the first ignition: a retarget, the same flame.
        q.fire(Some(at(10.0, 8.0)), t0 + MS * 208);
        let kept = q.fire(Some(at(10.0, 8.0)), t0 + MS * 220).expect("burning");
        assert_eq!(kept.ignition.seed, Seed(1), "no new ignition within 334 ms");
        assert!(
            (kept.ignition.age - 0.212 * FIRE_RATE).abs() < 0.001,
            "its age runs on, on the fire's clock: {}",
            kept.ignition.age
        );
        assert_eq!(kept.emitter.hole, at(10.0, 8.0), "the quad retargeted");
        // 412 ms after it: a new ignition, at age 0 where it was lit.
        q.fire(Some(at(90.0, 1.0)), t0 + MS * 408);
        let relit = q.fire(Some(at(90.0, 1.0)), t0 + MS * 420).expect("relit");
        assert_eq!(relit.ignition.seed, Seed(2), "a new ignition after 334 ms");
        assert!(
            relit.ignition.age.abs() < 0.001,
            "lit now: {}",
            relit.ignition.age
        );
    }

    /// **A landed flame waits where the chase rests until the next move is
    /// taken**, for every effect. A long move lights it, a step a cell on
    /// folds its quad in on the fast constant, and the quad lands while the
    /// ignition still burns, the shortest-lived -- the liquid's 0.22 s --
    /// included. Then the cursor makes a long move. Through that move's
    /// dwell the emitter stays on the cell the quad landed on, which is
    /// where `take` starts the next segment from, and the hole is the
    /// cursor's new cell. The frame that takes the move continues from
    /// there, every corner on its way down and none past the cursor. Drawn
    /// at the cursor instead, the emitter jumped to the new cell for the
    /// dwell and back to the old one when the move was taken -- on every
    /// line of a wheel scroll that carries a placed cursor down a row or
    /// more, as helix's does. The control: a smear has nothing to draw once
    /// it lands, so the quad draws nothing through the dwell.
    #[test]
    fn a_landed_flame_waits_where_the_chase_rests_until_the_next_move_is_taken() {
        let (from, lit, rest, next) = (at(10.0, 5.0), at(13.0, 5.0), at(14.0, 5.0), at(14.0, 9.0));
        for effect in Effect::ALL {
            let t0 = Instant::now();
            let mut q = Chase::new(Style::Effect(effect));
            assert!(
                q.fire(Some(from), t0).is_none(),
                "{effect:?}: first sighting"
            );
            q.fire(Some(lit), t0 + MS * 8);
            assert!(
                q.fire(Some(lit), t0 + MS * 20).is_some(),
                "{effect:?}: three cells is a long move, and it lights"
            );
            // A cell on, taken once its dwell is out: the same flame.
            q.fire(Some(rest), t0 + MS * 30);
            let folding = q
                .fire(Some(rest), t0 + MS * 42)
                .unwrap_or_else(|| panic!("{effect:?}: out at the step"));
            assert_eq!(
                folding.ignition.seed,
                Seed(1),
                "{effect:?}: no new ignition"
            );
            // The first tick the quad has landed: the effect still burns.
            let mut ms = 42;
            while q.moving.is_some() {
                ms += 2;
                q.fire(Some(rest), t0 + MS * ms);
            }
            let landed = q
                .fire(Some(rest), t0 + MS * ms)
                .unwrap_or_else(|| panic!("{effect:?}: out before its quad landed, at {ms} ms"));
            assert_eq!(landed.emitter.corners, rest.corners(), "{effect:?}: landed");
            // Four rows down, seen at `ms` and again 4 ms later: the dwell.
            for dwell in [0, 4] {
                let f = q
                    .fire(Some(next), t0 + MS * (ms + dwell))
                    .unwrap_or_else(|| panic!("{effect:?}: out in the dwell"));
                assert_eq!(
                    f.emitter.corners,
                    rest.corners(),
                    "{effect:?}: {dwell} ms into the dwell the flame stays where the chase rests"
                );
                assert_eq!(f.emitter.hole, next, "{effect:?}: the cursor is bare");
            }
            let took = q
                .fire(Some(next), t0 + MS * (ms + 12))
                .unwrap_or_else(|| panic!("{effect:?}: out when the move was taken"));
            assert!(q.moving.is_some(), "{effect:?}: the move was taken");
            for ((p, r), n) in took
                .emitter
                .corners
                .0
                .iter()
                .zip(rest.corners().0)
                .zip(next.corners().0)
            {
                assert!(
                    p.y >= r.y - 1e-3 && p.y < n.y,
                    "{effect:?}: the take continues from the rest, down toward the cursor: \
                     {p:?} between {r:?} and {n:?}"
                );
            }
        }
        for shape in [Shape::Quad, Shape::Sweep] {
            let t0 = Instant::now();
            let mut q = Chase::new(Style::Smear(shape));
            q.smear(Some(from), t0, CELL);
            q.smear(Some(rest), t0 + MS * 8, CELL);
            assert!(
                q.smear(Some(rest), t0 + MS * 20, CELL).is_some(),
                "{shape:?}"
            );
            let mut ms = 20;
            while q.smear(Some(rest), t0 + MS * ms, CELL).is_some() {
                ms += 2;
            }
            for dwell in [0, 4] {
                assert_eq!(
                    q.smear(Some(next), t0 + MS * (ms + dwell), CELL),
                    None,
                    "{shape:?}: a landed smear draws nothing in the dwell"
                );
            }
        }
    }

    /// **A restyle mid-fire owes nothing afterwards**, which is what Reduce
    /// Motion turning on does to a burning fire: the driver stops calling
    /// `frame` once the trail is off, so a restyle that kept the fire would
    /// owe a frame on every tick for ever. Ten seconds of ticks at 120 Hz.
    #[test]
    fn a_restyle_mid_fire_owes_nothing() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Effect(Effect::Fire));
        q.fire(Some(at(10.0, 5.0)), t0);
        q.fire(Some(at(60.0, 5.0)), t0 + MS * 8);
        q.fire(Some(at(60.0, 5.0)), t0 + MS * 20).expect("lit");
        let now = t0 + MS * 21;
        assert_eq!(q.owes(now), Owes::Frame, "the control: a fire owes frames");
        q.restyle(Style::Smear(Shape::Quad));
        for k in 0..1200 {
            let at = now + Duration::from_micros(8_333 * k);
            assert_eq!(q.owes(at), Owes::Nothing, "tick {k}");
        }
    }

    /// **A restyle keeps the ignition guard and the seed**, as a snap does:
    /// a switch away and back inside [`REIGNITE`] lights no fourth fire
    /// within a second, and the next ignition does not repeat the last
    /// one's seed.
    #[test]
    fn a_restyle_keeps_the_ignition_guard_and_the_seed() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Effect(Effect::Fire));
        q.fire(Some(at(10.0, 5.0)), t0);
        q.fire(Some(at(60.0, 5.0)), t0 + MS * 8);
        let first = q.fire(Some(at(60.0, 5.0)), t0 + MS * 20).expect("lit");
        assert_eq!(first.ignition.seed, Seed(1));
        q.restyle(Style::Smear(Shape::Quad));
        q.restyle(Style::Effect(Effect::Fire));
        // A first sighting, then a long move whose dwell ends inside the
        // window: refused, so nothing is drawn and nothing owed.
        assert_eq!(
            q.fire(Some(at(10.0, 9.0)), t0 + MS * 100),
            None,
            "first sighting"
        );
        q.fire(Some(at(90.0, 9.0)), t0 + MS * 110);
        assert_eq!(
            q.fire(Some(at(90.0, 9.0)), t0 + MS * 125),
            None,
            "no ignition within 334 ms of the last"
        );
        // Past the window: lit, and with the next seed.
        q.fire(Some(at(10.0, 1.0)), t0 + MS * 400);
        let relit = q.fire(Some(at(10.0, 1.0)), t0 + MS * 415).expect("relit");
        assert_eq!(relit.ignition.seed, Seed(2), "the seed runs on");
    }

    /// The control: a restyle at rest owes nothing, and neither did the
    /// chase before it.
    #[test]
    fn a_restyle_at_rest_owes_nothing() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Effect(Effect::Fire));
        assert_eq!(q.fire(Some(at(10.0, 5.0)), t0), None);
        assert_eq!(q.owes(t0 + MS), Owes::Nothing);
        q.restyle(Style::Smear(Shape::Quad));
        assert_eq!(q.owes(t0 + MS * 2), Owes::Nothing);
    }

    /// **An effect's axis is the move the quad is making**: the first move's
    /// on ignition, the retarget's when a long move inside [`REIGNITE`] turns
    /// the quad while the ignition keeps its first direction, and still the
    /// retarget's after the corners land -- where the quad's shape, its
    /// corners on the cursor, would say nothing.
    #[test]
    fn an_effects_axis_turns_with_the_quad_and_holds_when_it_lands() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Effect(Effect::Plasma));
        q.fire(Some(at(10.0, 5.0)), t0);
        q.fire(Some(at(60.0, 5.0)), t0 + MS * 8);
        let first = q.fire(Some(at(60.0, 5.0)), t0 + MS * 20).expect("lit");
        assert_eq!((first.axis.x, first.axis.y), (1.0, 0.0), "along the move");
        // A retarget straight down, inside the reignite window.
        q.fire(Some(at(60.0, 12.0)), t0 + MS * 100);
        let turned = q
            .fire(Some(at(60.0, 12.0)), t0 + MS * 120)
            .expect("burning");
        assert_eq!(turned.ignition.seed, Seed(1), "the same ignition");
        assert_eq!(
            (turned.ignition.dir.x, turned.ignition.dir.y),
            (1.0, 0.0),
            "which keeps its first direction"
        );
        assert!(
            turned.axis.y > 0.9,
            "the axis turned with the quad: {:?}",
            turned.axis
        );
        let landed = q
            .fire(Some(at(60.0, 12.0)), t0 + MS * 500)
            .expect("burning after the corners land");
        assert_eq!(landed.emitter.corners, at(60.0, 12.0).corners());
        assert_eq!(landed.axis, turned.axis, "and holds once they land");
    }

    /// **An effect lights at most three times a second even when it is out
    /// sooner**: a lightning bolt is gone in 0.32 s, inside [`REIGNITE`], and
    /// a long move 0.325 s after one lights none -- the guard is when the
    /// last was lit, not whether it still burns. The move it holds draws
    /// nothing and asks the tick for nothing either: with nothing burning
    /// there is nothing its quad would carry. One after the window lights.
    #[test]
    fn a_bolt_out_before_the_window_still_holds_it() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Effect(Effect::Lightning));
        q.fire(Some(at(10.0, 5.0)), t0);
        q.fire(Some(at(60.0, 5.0)), t0 + MS * 8);
        let first = q.fire(Some(at(60.0, 5.0)), t0 + MS * 20).expect("lit");
        assert_eq!(first.ignition.seed, Seed(1));
        assert_eq!(q.fire(Some(at(60.0, 5.0)), t0 + MS * 340), None, "out");
        // A long move lit 0.325 s after the first: no bolt.
        q.fire(Some(at(10.0, 12.0)), t0 + MS * 333);
        assert_eq!(q.fire(Some(at(10.0, 12.0)), t0 + MS * 345), None, "held");
        assert_eq!(q.owes(t0 + MS * 353), Owes::Nothing, "and owes no frame");
        // And one after the window lights.
        q.fire(Some(at(60.0, 20.0)), t0 + MS * 700);
        let next = q
            .fire(Some(at(60.0, 20.0)), t0 + MS * 712)
            .expect("lit after the window");
        assert_eq!(next.ignition.seed, Seed(2));
    }

    /// **Typing never lights a fire**: a move of two cells or fewer snaps,
    /// as it does for the quad, and a hidden cursor fades a burning one
    /// rather than putting it out.
    /// A snap forgets the fire and keeps counting seeds, so no two fires in a
    /// session burn alike, and keeps the guard, so no snap lets a fourth
    /// flash a second through.
    #[test]
    fn typing_never_lights_a_fire_and_a_snap_puts_one_out() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Effect(Effect::Fire));
        q.fire(Some(at(10.0, 5.0)), t0);
        for (i, col) in [11.0, 12.0, 14.0].into_iter().enumerate() {
            let now = t0 + MS * (20 * (u32::try_from(i).unwrap() + 1));
            assert_eq!(
                q.fire(Some(at(col, 5.0)), now),
                None,
                "a step to column {col}"
            );
            assert_eq!(q.owes(now + MS * 12), Owes::Nothing, "and nothing owed");
        }
        q.fire(Some(at(60.0, 5.0)), t0 + MS * 100);
        assert!(
            q.fire(Some(at(60.0, 5.0)), t0 + MS * 112).is_some(),
            "a long move lights"
        );
        let faded = q.fire(None, t0 + MS * 120).expect("hidden: fading");
        assert!(
            faded.emitter.opacity.get() < Smear::OPAQUE.get(),
            "hidden: faded, {:?}",
            faded.emitter.opacity
        );
        assert_eq!(
            q.fire(Some(at(60.0, 5.0)), t0 + MS * 128)
                .map(|f| f.ignition.seed),
            Some(Seed(1)),
            "shown again: the same flame"
        );
        // A long move 36 ms after the first was lit, across the hide: the
        // quad retargets and keeps the flame that burns, as inside the
        // window it does -- the guard is when the last was lit, and a hide
        // is no reason to flash again.
        q.fire(Some(at(10.0, 1.0)), t0 + MS * 136);
        assert_eq!(
            q.fire(Some(at(10.0, 1.0)), t0 + MS * 148)
                .map(|f| f.ignition.seed),
            Some(Seed(1)),
            "held"
        );
        q.fire(Some(at(60.0, 1.0)), t0 + MS * 500);
        assert_eq!(
            q.fire(Some(at(60.0, 1.0)), t0 + MS * 512)
                .map(|f| f.ignition.seed),
            Some(Seed(2))
        );
        q.snap();
        assert_eq!(q.owes(t0 + MS * 514), Owes::Nothing, "a snap puts it out");
        // A snap keeps the guard as well as the seeds: a long move after it
        // inside the window lights nothing, and one after the window does.
        q.fire(Some(at(10.0, 1.0)), t0 + MS * 520);
        q.fire(Some(at(80.0, 1.0)), t0 + MS * 528);
        assert_eq!(q.fire(Some(at(80.0, 1.0)), t0 + MS * 540), None, "held");
        q.fire(Some(at(10.0, 9.0)), t0 + MS * 900);
        assert_eq!(
            q.fire(Some(at(10.0, 9.0)), t0 + MS * 912)
                .map(|f| f.ignition.seed),
            Some(Seed(3)),
            "the seeds count on across a snap"
        );
    }

    impl Chase<Instant> {
        /// The frame's trail as a smear, for the quad's and the sweep's
        /// tests.
        fn smear(&mut self, cursor: Option<Rect>, now: Instant, cell: (f32, f32)) -> Option<Smear> {
            match self.frame(cursor, now, cell, PT) {
                Some(TrailLook::Smear(s)) => Some(s),
                None => None,
                Some(other) => panic!("a smear chase drew {other:?}"),
            }
        }
    }

    fn at(col: f32, row: f32) -> Rect {
        Rect {
            left: col * CELL.0,
            top: row * CELL.1,
            right: (col + 1.0) * CELL.0,
            bottom: (row + 1.0) * CELL.1,
        }
    }

    const MS: Duration = Duration::from_millis(1);

    /// The cursor seen at `to` on the ticks given, in ms after `t0`: the
    /// trail each of those frames draws.
    fn after_move(
        q: &mut Chase<Instant>,
        t0: Instant,
        to: Rect,
        ticks: &[u32],
    ) -> Vec<Option<Smear>> {
        ticks
            .iter()
            .map(|&t| q.smear(Some(to), t0 + MS * t, CELL))
            .collect()
    }

    /// A quad or a sweep in flight: a long move from (10, 5) to (60, 5),
    /// seen at `t0` and at 8 ms, taken at 18 and backdated to 8.
    fn in_flight(style: Style, t0: Instant) -> Chase<Instant> {
        let mut q = Chase::new(style);
        assert_eq!(q.frame(Some(at(10.0, 5.0)), t0, CELL, PT), None);
        assert_eq!(q.frame(Some(at(60.0, 5.0)), t0 + MS * 8, CELL, PT), None);
        assert!(
            q.frame(Some(at(60.0, 5.0)), t0 + MS * 18, CELL, PT)
                .is_some()
        );
        q
    }

    const FLOOR: f32 = -50.0 * CELL.1;

    fn moved_by(a: Corners, b: Corners, dy: f32) -> bool {
        a.0.iter()
            .zip(b.0)
            .all(|(p, q)| (q.x - p.x).abs() < 1e-3 && (q.y - p.y - dy).abs() < 1e-3)
    }

    /// **A ride rebases the move where its corners are**, moved by the
    /// scroll, from now, on its own constants and to the cursor it chased.
    #[test]
    fn a_ride_rebases_the_segment_and_keeps_its_constants() {
        let t0 = Instant::now();
        for style in [Style::Smear(Shape::Quad), Style::Smear(Shape::Sweep)] {
            let mut q = in_flight(style, t0);
            let (seg, s0) = q.moving.expect("in flight");
            let now = t0 + MS * 50;
            let before = seg.at(now.elapsed_since(s0).as_secs_f32());
            q.ride(-CELL.1, now, FLOOR, CELL.1);
            let (next, s1) = q.moving.expect("still in flight");
            assert_eq!(s1, now, "{style:?}: rebased at the ride");
            assert!(moved_by(before, next.from, -CELL.1), "{style:?}");
            assert_eq!((next.tau, next.to), (seg.tau, seg.to), "{style:?}");
            assert_eq!(q.carry, Carry::Riding(-CELL.1));
            assert_eq!(q.target, Some(at(60.0, 5.0)), "the rest state never moves");
        }
    }

    /// **A ridden trail still lands on the cursor**, and owes nothing after.
    #[test]
    fn a_ridden_trail_lands_on_the_cursor() {
        let t0 = Instant::now();
        let to = at(60.0, 5.0);
        let mut q = in_flight(Style::Smear(Shape::Quad), t0);
        q.ride(-CELL.1, t0 + MS * 40, FLOOR, CELL.1);
        let mut last = None;
        for ms in (48..1500).step_by(8) {
            match q.smear(Some(to), t0 + MS * ms, CELL) {
                Some(s) => last = Some(s),
                None => break,
            }
        }
        let last = last.expect("it drew after the ride");
        let landed = last
            .corners
            .0
            .iter()
            .zip(to.corners().0)
            .all(|(p, c)| (p.x - c.x).abs() < 2.0 && (p.y - c.y).abs() < 2.0);
        assert!(
            landed,
            "its last frame is on the cursor: {:?}",
            last.corners
        );
        assert_eq!(q.owes(t0 + MS * 1500), Owes::Nothing);
    }

    /// **At rest a scroll moves nothing**: no move in flight, the place the
    /// cursor was taken kept, and the frame after owes nothing.
    #[test]
    fn a_landed_trail_rides_nothing_and_scroll_is_no_move() {
        let t0 = Instant::now();
        let to = at(60.0, 5.0);
        let mut q = in_flight(Style::Smear(Shape::Quad), t0);
        let mut ms = 18;
        while q.smear(Some(to), t0 + MS * ms, CELL).is_some() {
            ms += 8;
        }
        q.ride(-CELL.1, t0 + MS * ms, FLOOR, CELL.1);
        assert!(q.moving.is_none());
        assert_eq!(q.target, Some(to));
        assert_eq!(q.smear(Some(to), t0 + MS * (ms + 8), CELL), None);
        assert_eq!(q.owes(t0 + MS * (ms + 8)), Owes::Nothing);
    }

    /// **Two rides at one instant are one ride of their sum.**
    #[test]
    fn two_rides_at_one_instant_are_one_ride_of_their_sum() {
        let t0 = Instant::now();
        let now = t0 + MS * 60;
        let mut a = in_flight(Style::Smear(Shape::Quad), t0);
        let mut b = in_flight(Style::Smear(Shape::Quad), t0);
        a.ride(-20.0, now, FLOOR, CELL.1);
        a.ride(-30.0, now, FLOOR, CELL.1);
        b.ride(-50.0, now, FLOOR, CELL.1);
        let (sa, ta) = a.moving.unwrap();
        let (sb, tb) = b.moving.unwrap();
        assert_eq!(ta, tb);
        assert!(moved_by(sa.from, sb.from, 0.0), "{sa:?} against {sb:?}");
        assert_eq!((sa.tau, a.carry), (sb.tau, b.carry));
    }

    /// **Nothing moves past one window above the top**: a corner a ride
    /// would take above the floor stops at it, while the rest ride on.
    #[test]
    fn a_ride_stops_one_window_above_the_top() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Quad));
        let from = Corners([
            Pt { x: 100.0, y: 10.0 },
            Pt { x: 100.0, y: 300.0 },
            Pt { x: 50.0, y: 300.0 },
            Pt { x: 50.0, y: 10.0 },
        ]);
        q.moving = Some((Segment::new(from, at(60.0, 5.0)), t0));
        q.ride(-100.0, t0, -60.0, CELL.1);
        let (seg, _) = q.moving.expect("kept: two corners are on the window");
        let ys: Vec<f32> = seg.from.0.iter().map(|p| p.y).collect();
        assert_eq!(ys, [-60.0, 200.0, 200.0, -60.0]);
    }

    /// **A move is dropped once every corner is above the leaving row** --
    /// the row the slide draws above the window, `cell_h` up -- by half a
    /// pixel, the reach of its edge; one still inside it is kept.
    #[test]
    fn a_move_above_the_leaving_row_is_dropped() {
        let t0 = Instant::now();
        let flat = Corners([
            Pt { x: 100.0, y: 20.0 },
            Pt { x: 100.0, y: 20.0 },
            Pt { x: 50.0, y: 20.0 },
            Pt { x: 50.0, y: 20.0 },
        ]);
        let mut kept = Chase::new(Style::Smear(Shape::Quad));
        kept.moving = Some((Segment::new(flat, at(60.0, 5.0)), t0));
        kept.ride(-54.4, t0, FLOOR, CELL.1);
        assert!(
            kept.moving.is_some(),
            "at -34.4 it still shows in the leaving row"
        );
        let mut gone = Chase::new(Style::Smear(Shape::Quad));
        gone.moving = Some((Segment::new(flat, at(60.0, 5.0)), t0));
        gone.ride(-54.6, t0, FLOOR, CELL.1);
        assert!(gone.moving.is_none(), "at -34.6 nothing of it shows");
    }

    /// **A ridden trail lands while output streams**: a line a frame at 60
    /// Hz. A quad and a sweep in flight ride the first two rows and fold in
    /// on the third, landing within 0.2 s of it; the chase then owes
    /// nothing, however long the stream goes on. Unbounded, the ride keeps
    /// the slow corners a row or more above the cursor and owes a frame on
    /// every tick for the whole stream.
    #[test]
    fn a_ridden_trail_lands_while_output_streams() {
        let t0 = Instant::now();
        let to = at(60.0, 5.0);
        for style in [Style::Smear(Shape::Quad), Style::Smear(Shape::Sweep)] {
            let mut q = in_flight(style, t0);
            let (mut rides, mut third) = (0u32, None);
            let mut landed = None;
            for tick in 3..=240u32 {
                let now = t0 + Duration::from_micros(u64::from(tick) * 8333);
                if tick % 2 == 0 {
                    q.ride(-CELL.1, now, FLOOR, CELL.1);
                    rides += 1;
                    if rides == 3 {
                        third = Some(now);
                    }
                }
                let drew = q.frame(Some(to), now, CELL, PT).is_some();
                if !drew && landed.is_none() && third.is_some() {
                    landed = Some(now);
                }
                if let Some(l) = landed {
                    assert_eq!(
                        q.owes(now),
                        Owes::Nothing,
                        "{style:?}: owes after landing at {:?}",
                        l - t0
                    );
                }
            }
            let (third, landed) = (
                third.unwrap(),
                landed.expect("it landed while the stream ran"),
            );
            assert!(
                landed - third <= MS * 200,
                "{style:?}: landed {:?} after its third ride",
                landed - third
            );
        }
    }

    /// **A new move rides again after a fold**: the carry is the move's, so a
    /// move taken after one that folded has its own two rows to ride.
    #[test]
    fn a_new_move_rides_again_after_a_fold() {
        let t0 = Instant::now();
        let mut q = in_flight(Style::Smear(Shape::Quad), t0);
        q.ride(-3.0 * CELL.1, t0 + MS * 30, FLOOR, CELL.1);
        assert_eq!(q.carry, Carry::Folded, "three rows is past the bound");
        let far = at(10.0, 30.0);
        q.frame(Some(far), t0 + MS * 40, CELL, PT);
        assert!(q.frame(Some(far), t0 + MS * 52, CELL, PT).is_some());
        let (seg, s0) = q.moving.expect("the new move in flight");
        assert_eq!(seg.to, far);
        let now = t0 + MS * 60;
        let before = seg.at(now.elapsed_since(s0).as_secs_f32());
        q.ride(-CELL.1, now, FLOOR, CELL.1);
        let (next, _) = q.moving.expect("still in flight");
        assert!(moved_by(before, next.from, -CELL.1), "the new move rode");
        assert_eq!(q.carry, Carry::Riding(-CELL.1));
    }

    /// **The drawn trail moves as the text does across a retire** -- the ride
    /// and the slide's lift together, as `pump` and `present` compose them
    /// (note X's test 10). At the instant a read retires `n` lines, from a
    /// settled slide and from one mid-slide, every part drawn from the path
    /// -- a quad's corners, a fire's path -- moves by what the text drawn
    /// moves, `-n*h` and the pan's change, and the hole by what the drawn
    /// cursor moves, the pan's change alone. Over the slide that follows, the
    /// fire's path stays with the text as the pan drains. With neither half
    /// only one line from a settled slide holds; with the lift alone, only
    /// the slide that follows.
    #[test]
    fn the_drawn_trail_moves_as_the_text_does_across_a_retire() {
        let h = CELL.1;
        let parts = |l: TrailLook| match l {
            TrailLook::Smear(s) => (s.corners, s.hole, None),
            TrailLook::Vfx(v) => (v.emitter.corners, v.emitter.hole, Some(v.ignition.tail)),
            TrailLook::Ghost(_) => panic!("a chase draws no ghost"),
        };
        let to = at(60.0, 40.0);
        let drawn = |q: &mut Chase<Instant>, slide: &Slide, now: Instant| {
            parts(
                q.frame(Some(to), now, CELL, PT)
                    .expect("in flight")
                    .lifted(slide.pan()),
            )
        };
        let px = |s: &Slide| s.pan().value() as f32;
        for style in [Style::Smear(Shape::Quad), Style::Effect(Effect::Fire)] {
            for n in [1usize, 2, 5] {
                for settled in [true, false] {
                    let case = format!("{style:?}, {n} lines, settled {settled}");
                    let t0 = Instant::now();
                    let mut q = Chase::new(style);
                    q.frame(Some(at(10.0, 44.0)), t0, CELL, PT);
                    q.frame(Some(to), t0 + MS * 8, CELL, PT);
                    let mut slide = Slide::new(Px::of(34));
                    if !settled {
                        slide.retire(Rows::of_count(1));
                        slide.advance(0.02);
                    }
                    let t = t0 + MS * 40;
                    let (c0, h0, tail0) = drawn(&mut q, &slide, t);
                    let pan0 = px(&slide);
                    slide.retire(Rows::of_count(n));
                    q.ride(-(n as f32) * h, t, FLOOR, h);
                    let (c1, h1, tail1) = drawn(&mut q, &slide, t);
                    let dpan = px(&slide) - pan0;
                    let text = -(n as f32) * h + dpan;
                    assert!(moved_by(c0, c1, text), "{case}: the corners left the text");
                    assert!(
                        (h1.top - h0.top - dpan).abs() < 1e-3 && h1.left == h0.left,
                        "{case}: the hole left the drawn cursor"
                    );
                    if let (Some(a), Some(b)) = (tail0, tail1) {
                        assert!(
                            (b.y - a.y - text).abs() < 1e-3 && b.x == a.x,
                            "{case}: the path left the text"
                        );
                        let (mut last, mut pan_last) = (b, px(&slide));
                        for k in 1..=6u32 {
                            slide.advance(0.01);
                            let (_, _, tail) = drawn(&mut q, &slide, t + MS * (10 * k));
                            let tail = tail.expect("still burning");
                            assert!(
                                (tail.y - last.y - (px(&slide) - pan_last)).abs() < 1e-3,
                                "{case}: the path left the text as the slide drained"
                            );
                            (last, pan_last) = (tail, px(&slide));
                        }
                    }
                }
            }
        }
    }

    /// **A fire's path rides whole and its life is kept**: the tail moves and
    /// the ride is recorded for the noise; when it was lit, when it goes out
    /// and its seed do not move. Past the floor the path stops, and the ride
    /// records what was applied.
    #[test]
    fn a_fire_rides_its_path_and_keeps_its_life() {
        let t0 = Instant::now();
        let (from, to) = (at(10.0, 5.0), at(60.0, 5.0));
        let mut q = Chase::new(Style::Effect(Effect::Fire));
        q.fire(Some(from), t0);
        q.fire(Some(to), t0 + MS * 8);
        let before = q.fire(Some(to), t0 + MS * 20).expect("lit");
        let (at0, end0) = q.fire.map(|b| (b.at, b.end)).unwrap();
        q.ride(-CELL.1, t0 + MS * 30, FLOOR, CELL.1);
        let after = q.fire(Some(to), t0 + MS * 40).expect("still burning");
        assert!((after.ignition.tail.y - before.ignition.tail.y + CELL.1).abs() < 1e-3);
        assert_eq!(after.ignition.tail.x, before.ignition.tail.x);
        assert_eq!(after.ignition.rode, -CELL.1);
        assert_eq!(
            (after.ignition.dir, after.ignition.len),
            (before.ignition.dir, before.ignition.len)
        );
        assert_eq!(after.ignition.seed, before.ignition.seed);
        assert_eq!(q.fire.map(|b| (b.at, b.end)), Some((at0, end0)));
        // To the floor and no further: the tail is about 5.5 rows down.
        let tail = after.ignition.tail.y;
        q.ride(-1000.0, t0 + MS * 50, -100.0, CELL.1);
        let past = q.fire.expect("still burning").ignition;
        assert_eq!(past.tail.y, -100.0);
        assert!(
            (past.rode - (-CELL.1 + (-100.0 - tail))).abs() < 1e-3,
            "{}",
            past.rode
        );
    }

    /// **A sweep takes the same moves on sfte's one constant**: the tail it
    /// draws is the cursor's rectangle, a cell whole at every frame, and it
    /// is an e-fold of the way in at 100 ms after the move started, where a
    /// quad's leading corner is already there and the quad stretched out
    /// behind it.
    #[test]
    fn a_sweep_takes_the_same_moves_on_sftes_constant() {
        let t0 = Instant::now();
        let (from, to) = (at(10.0, 5.0), at(60.0, 5.0));
        let mut sweep = Chase::new(Style::Smear(Shape::Sweep));
        let mut quad = Chase::new(Style::Smear(Shape::Quad));
        for c in [&mut sweep, &mut quad] {
            assert_eq!(c.smear(Some(from), t0, CELL), None, "first sighting");
            assert_eq!(c.smear(Some(to), t0 + MS * 8, CELL), None, "the dwell");
        }
        // Started at 8 ms, backdated from the dwell's end at 18.
        let s = sweep.smear(Some(to), t0 + MS * 108, CELL).expect("a sweep");
        let q = quad.smear(Some(to), t0 + MS * 108, CELL).expect("a quad");
        assert_eq!((s.shape, q.shape), (Shape::Sweep, Shape::Quad));
        let c = s.corners.0;
        assert!(
            (c[1].x - c[3].x - CELL.0).abs() < 0.001 && (c[1].y - c[3].y - CELL.1).abs() < 0.001,
            "the tail is a cell: {c:?}"
        );
        let left = to.left - c[3].x;
        let gap = to.left - from.left;
        assert!(
            (left - gap * (-1.0f32).det_exp()).abs() < 1.0,
            "an e-fold in at 100 ms: {left} of {gap}"
        );
        let qc = q.corners.0;
        assert!(
            (to.right - qc[0].x) < 1.0 && qc[0].x - qc[3].x > CELL.0 + 100.0,
            "the quad's front has arrived and it is stretched out behind: {qc:?}"
        );
        // A step in flight folds it in on the fast constant, as a quad's: on
        // ticks every 8 ms it is over by 250 ms, where the sweep left alone
        // runs to 752 ms.
        let mut alone = Chase::new(Style::Smear(Shape::Sweep));
        alone.smear(Some(from), t0, CELL);
        alone.smear(Some(to), t0 + MS * 8, CELL);
        let mut last = None;
        for ms in (116..=250).step_by(8) {
            last = sweep.smear(Some(at(61.0, 5.0)), t0 + MS * ms, CELL);
            assert!(
                alone.smear(Some(to), t0 + MS * ms, CELL).is_some(),
                "{ms} ms"
            );
        }
        assert_eq!(last, None, "the step folded the sweep in: {last:?}");
        assert!(
            alone.smear(Some(to), t0 + MS * 740, CELL).is_some(),
            "still in at 740 ms"
        );
        assert_eq!(
            alone.smear(Some(to), t0 + MS * 760, CELL),
            None,
            "over at 760 ms"
        );
        // A snap forgets the cursor and keeps the shape.
        alone.snap();
        alone.smear(Some(from), t0 + MS * 800, CELL);
        alone.smear(Some(to), t0 + MS * 808, CELL);
        let again = alone
            .smear(Some(to), t0 + MS * 820, CELL)
            .expect("a sweep after the snap");
        assert_eq!(again.shape, Shape::Sweep);
    }

    /// **A long move waits out the dwell and then trails**, backdated to when
    /// the cursor was first seen there: the first frame at or past the dwell
    /// draws a quad already on its way, and the tick owes that frame.
    #[test]
    fn a_long_move_trails_once_the_dwell_is_out() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Quad));
        assert_eq!(
            q.smear(Some(at(10.0, 5.0)), t0, CELL),
            None,
            "first sighting"
        );
        let frames = after_move(&mut q, t0, at(60.0, 5.0), &[8, 16, 18]);
        assert_eq!(
            frames[0], None,
            "first seen at 60 at 8 ms: its dwell starts"
        );
        assert_eq!(
            frames[1], None,
            "the move is still inside its dwell at 16 ms"
        );
        assert_eq!(
            q.owes(t0 + MS * 19),
            Owes::Frame,
            "a moving trail owes frames"
        );
        let s = frames[2].expect("the dwell ran out at 18 ms; the trail started");
        // Backdated to 8 ms, when the cursor was first seen at 60: the
        // leading corner is 8 ms along, not at its start.
        assert!(
            s.corners.0[0].x > at(10.0, 5.0).right + 1.0,
            "the trail's first frame is already on its way: {:?}",
            s.corners.0[0]
        );
        assert_eq!(
            s.hole,
            at(60.0, 5.0),
            "the cursor's own rectangle is spared"
        );
    }

    /// **A move inside its dwell owes the tick the frame the dwell ends on**,
    /// and nothing before it: after a lone keystroke's echo nothing else
    /// draws, so without this the trail would never start.
    #[test]
    fn the_dwell_owes_exactly_the_frame_it_ends_on() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Quad));
        q.smear(Some(at(0.0, 0.0)), t0, CELL);
        q.smear(Some(at(40.0, 3.0)), t0 + MS * 1, CELL);
        assert_eq!(q.owes(t0 + MS * 5), Owes::Nothing);
        assert_eq!(q.owes(t0 + MS * 11), Owes::Frame);
    }

    /// **A short move from rest snaps**, on both axes, and one just past the
    /// threshold trails.
    #[test]
    fn two_cells_or_fewer_from_rest_snap() {
        for (to, trails) in [
            (at(12.0, 5.0), false),
            (at(10.0, 7.0), false),
            (at(12.0, 7.0), false),
            (at(13.0, 5.0), true),
            (at(10.0, 8.0), true),
        ] {
            let t0 = Instant::now();
            let mut q = Chase::new(Style::Smear(Shape::Quad));
            q.smear(Some(at(10.0, 5.0)), t0, CELL);
            let frames = after_move(&mut q, t0, to, &[1, 20, 40]);
            assert_eq!(frames[2].is_some(), trails, "to {to:?}");
            if !trails {
                assert_eq!(
                    q.owes(t0 + MS * 41),
                    Owes::Nothing,
                    "a snap leaves nothing owed"
                );
            }
        }
    }

    /// **A cursor that never rests is never followed**: moving on every
    /// frame, as streaming output does, it starts no trail; once it stops,
    /// the trail runs from where it last rested.
    #[test]
    fn a_cursor_that_never_rests_is_not_followed() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Quad));
        q.smear(Some(at(0.0, 0.0)), t0, CELL);
        for i in 1..30u32 {
            let r = at(f32::from(u16::try_from(i * 5 % 190).unwrap()), 40.0);
            assert_eq!(q.smear(Some(r), t0 + MS * (i * 8), CELL), None, "frame {i}");
        }
        let rest = at(3.0, 40.0);
        q.smear(Some(rest), t0 + MS * 240, CELL);
        let s = q
            .smear(Some(rest), t0 + MS * 260, CELL)
            .expect("it trails once the cursor rests");
        assert_eq!(s.hole, rest);
        // From the rectangle the cursor last rested at, row 0.
        assert!(s.corners.0[3].y < rest.top, "the trail comes from above");
    }

    /// **A retarget in flight keeps the corners where they are**: the frame
    /// after the new move is taken continues from the old segment's corners
    /// at that instant, not from the rectangle the old move started at.
    #[test]
    fn a_retarget_in_flight_is_continuous() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Quad));
        q.smear(Some(at(0.0, 0.0)), t0, CELL);
        q.smear(Some(at(100.0, 0.0)), t0 + MS * 1, CELL);
        assert!(q.smear(Some(at(100.0, 0.0)), t0 + MS * 60, CELL).is_some());
        // A new place, seen at 60 and taken once its dwell is out, at 72.
        // Inside the dwell the trail still chases the old place, and spares
        // the cursor where it now is.
        let waiting = q.smear(Some(at(100.0, 30.0)), t0 + MS * 60, CELL).unwrap();
        assert_eq!(
            waiting.hole,
            at(100.0, 30.0),
            "the hole is the cursor this frame"
        );
        let (old, old_t0) = q.moving.unwrap();
        let after = q.smear(Some(at(100.0, 30.0)), t0 + MS * 72, CELL).unwrap();
        let (seg, t) = q.moving.unwrap();
        assert_eq!(t, t0 + MS * 72, "a retarget starts when it is taken");
        assert_ne!(seg, old, "the move was retargeted");
        // Continuous: the new move starts exactly where the old one was at 72.
        let was = old.at((t0 + MS * 72).elapsed_since(old_t0).as_secs_f32());
        assert_eq!(after.corners, was);
        assert_eq!(seg.from, was);
    }

    /// **A trail ends**, and then owes nothing: the last corner arrives within
    /// half a pixel, the frame after draws none, and an idle cursor costs no
    /// frames.
    #[test]
    fn a_trail_ends_and_owes_nothing_after() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Quad));
        q.smear(Some(at(0.0, 0.0)), t0, CELL);
        let to = at(90.0, 0.0);
        q.smear(Some(to), t0 + MS * 1, CELL);
        assert!(q.smear(Some(to), t0 + MS * 20, CELL).is_some());
        let (seg, _) = q.moving.unwrap();
        let end = t0 + MS + Duration::from_secs_f32(seg.lasts());
        assert!(q.smear(Some(to), end + MS, CELL).is_none());
        assert_eq!(q.owes(end + MS * 2), Owes::Nothing);
    }

    /// **A short move from rest snaps at once and owes nothing**: a snap
    /// draws nothing, so it waits for no dwell and asks the tick for no
    /// frame -- typing costs the frames its echoes cost and no more.
    #[test]
    fn a_short_move_from_rest_snaps_at_once_and_owes_nothing() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Quad));
        q.smear(Some(at(10.0, 5.0)), t0, CELL);
        for (i, col) in [11.0f32, 12.0, 14.0, 16.0].into_iter().enumerate() {
            let t = t0 + MS * 100 * (u32::try_from(i).unwrap() + 1);
            assert_eq!(
                q.smear(Some(at(col, 5.0)), t, CELL),
                None,
                "to column {col}"
            );
            assert_eq!(q.owes(t + MS * 20), Owes::Nothing, "to column {col}");
        }
        // Each snap moved the rest, so a run of short moves never adds up to
        // a long one: column 16 is six from where it started and snapped.
        assert_eq!(q.target, Some(at(16.0, 5.0)));
    }

    /// **Typing after a jump folds the trail in**: a keystroke's step while a
    /// trail is in flight puts every corner on the fast constant, so the
    /// trail ends inside the first keystroke's interval and the steps after
    /// it snap.
    #[test]
    fn typing_after_a_jump_folds_the_trail_in() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Quad));
        q.smear(Some(at(50.0, 5.0)), t0, CELL);
        // A jump to the start of the next line, as a newline makes it.
        q.smear(Some(at(0.0, 6.0)), t0 + MS * 1, CELL);
        assert!(q.smear(Some(at(0.0, 6.0)), t0 + MS * 20, CELL).is_some());
        let mut ended = None;
        for k in 1..20u32 {
            let key = t0 + MS * (20 + 100 * k);
            let col = f32::from(u16::try_from(k).unwrap());
            q.smear(Some(at(col, 6.0)), key, CELL);
            // The ticks until the next key.
            for tick in (12..100).step_by(8) {
                q.smear(Some(at(col, 6.0)), key + MS * tick, CELL);
            }
            if q.moving.is_none() && ended.is_none() {
                ended = Some(k);
            }
        }
        let ended = ended.expect("the trail never ended while typing");
        // Inside the first keystroke's interval: on the fast constant the
        // jump's tail folds in before the second key, where kitty's slow one
        // would carry it into the third.
        assert_eq!(ended, 1, "the trail outlived {ended} keystrokes");
        assert_eq!(q.owes(t0 + MS * 5000), Owes::Nothing);
    }

    /// **Holding a key after a jump ends the trail too**: at a key repeat's
    /// 30 ms, faster than a one-cell step's fold, the steps keep arriving
    /// while the last is in flight -- so once every corner has folded to
    /// within the threshold of the cursor, a short step snaps rather than
    /// animating the next cell, and the frames stop while the key is held.
    #[test]
    fn holding_a_key_after_a_jump_ends_the_trail() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Quad));
        q.smear(Some(at(50.0, 5.0)), t0, CELL);
        q.smear(Some(at(0.0, 6.0)), t0 + MS * 1, CELL);
        assert!(q.smear(Some(at(0.0, 6.0)), t0 + MS * 20, CELL).is_some());
        let mut ended = None;
        for k in 1..40u32 {
            let key = t0 + MS * (20 + 30 * k);
            let col = f32::from(u16::try_from(k).unwrap());
            q.smear(Some(at(col, 6.0)), key, CELL);
            for tick in [8u32, 16, 24] {
                q.smear(Some(at(col, 6.0)), key + MS * tick, CELL);
            }
            if q.moving.is_none() && ended.is_none() {
                ended = Some(k);
            }
            if let Some(e) = ended {
                assert!(
                    q.moving.is_none(),
                    "a held key restarted the trail at {k}, after {e}"
                );
            }
        }
        let ended = ended.expect("the trail never ended while the key was held");
        assert!(ended <= 8, "the trail outlived {ended} repeats");
    }

    /// **A hidden cursor fades its trail, and a snap forgets where it
    /// was**: hidden in flight, the quad is drawn faded and owes its
    /// frames; a snap puts it out at once, and the sighting after a snap
    /// starts nothing.
    #[test]
    fn hiding_fades_the_trail_and_a_snap_forgets() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Quad));
        q.smear(Some(at(0.0, 0.0)), t0, CELL);
        q.smear(Some(at(50.0, 9.0)), t0 + MS * 1, CELL);
        assert!(q.smear(Some(at(50.0, 9.0)), t0 + MS * 20, CELL).is_some());
        let faded = q.smear(None, t0 + MS * 28, CELL).expect("hidden: fading");
        assert!(faded.opacity.get() < Smear::OPAQUE.get(), "hidden: faded");
        assert_eq!(q.owes(t0 + MS * 29), Owes::Frame, "the fade's frames");
        q.snap();
        assert_eq!(q.owes(t0 + MS * 30), Owes::Nothing, "a snap puts it out");
        q.smear(Some(at(0.0, 0.0)), t0 + MS * 40, CELL);
        assert!(
            q.smear(Some(at(0.0, 0.0)), t0 + MS * 60, CELL).is_none(),
            "a first sighting"
        );
    }

    /// The opacity a look is drawn at: a smear's, or an effect's emitter's.
    fn opacity(look: &TrailLook) -> f32 {
        let chan = match look {
            TrailLook::Smear(s) => s.opacity,
            TrailLook::Vfx(v) => v.emitter.opacity,
            TrailLook::Ghost(_) => Smear::OPAQUE,
        };
        f32::from(chan.get()) / 255.0
    }

    /// The hole a look leaves bare.
    fn hole(look: &TrailLook) -> Option<Rect> {
        match look {
            TrailLook::Smear(s) => Some(s.hole),
            TrailLook::Vfx(v) => Some(v.emitter.hole),
            TrailLook::Ghost(_) => None,
        }
    }

    /// Whether `got` is `want` to the nearest of the 255 steps a look's
    /// opacity is drawn in.
    fn at_level(got: f32, want: f32) -> bool {
        (got - want).abs() <= 0.5 / 255.0 + 1e-6
    }

    /// A long move from (0, 0) to (90, 20), seen at `t0` and 2 ms on and
    /// taken at 20 ms, backdated to 2: what that frame draws.
    fn jumped(q: &mut Chase<Instant>, t0: Instant) -> TrailLook {
        q.frame(Some(at(0.0, 0.0)), t0, CELL, PT);
        q.frame(Some(at(90.0, 20.0)), t0 + MS * 2, CELL, PT);
        q.frame(Some(at(90.0, 20.0)), t0 + MS * 20, CELL, PT)
            .expect("taken")
    }

    /// **A hidden cursor's trail fades out over the slow constant**, as
    /// kitty's does (`[[source: kitty-cursor-trail]]` 64, 134-139): from the
    /// last frame the cursor was shown on, each frame draws it at
    /// `1 - t / FADE`, its corners chasing on and an effect ageing on, the
    /// hole where the cursor was; at nothing it is out and owes nothing.
    /// The quad, the sweep and the ufo, each of which outlasts the fade on
    /// this move, at 60 frames a second. Perturbation: the fade time
    /// halved -- the trail is out half way.
    #[test]
    fn a_hidden_cursors_trail_fades_out_over_the_slow_constant() {
        let t0 = Instant::now();
        let shown = t0 + MS * 20;
        let to = at(90.0, 20.0);
        for style in [
            Style::Smear(Shape::Quad),
            Style::Smear(Shape::Sweep),
            Style::Effect(Effect::Ufo),
        ] {
            let mut q = Chase::new(style);
            let mut last = jumped(&mut q, t0);
            assert_eq!(opacity(&last), 1.0, "{style:?}: shown, whole");
            for k in 1..=24u32 {
                let now = shown + MS * (16 * k);
                let look = q
                    .frame(None, now, CELL, PT)
                    .unwrap_or_else(|| panic!("{style:?}: out {} ms into the fade", 16 * k));
                let want = 1.0 - (now - shown).as_secs_f32() / FADE;
                let got = opacity(&look);
                assert!(
                    at_level(got, want),
                    "{style:?}, {} ms: drawn at {got}, not {want}",
                    16 * k
                );
                match (&look, &last) {
                    (TrailLook::Smear(a), TrailLook::Smear(b)) => {
                        assert_ne!(a.corners, b.corners, "{style:?}: the corners chase on");
                    }
                    (TrailLook::Vfx(a), TrailLook::Vfx(b)) => {
                        assert!(a.ignition.age > b.ignition.age, "{style:?}: it ages on");
                    }
                    _ => panic!("{style:?}: drew {look:?}"),
                }
                assert_eq!(hole(&look), Some(to), "{style:?}: the hole stays");
                assert_eq!(q.owes(now), Owes::Frame, "{style:?}: owed");
                last = look;
            }
            let out = shown + MS * 400;
            assert_eq!(q.frame(None, out, CELL, PT), None, "{style:?}: out");
            assert_eq!(q.owes(out + MS), Owes::Nothing, "{style:?}: owes nothing");
            assert_eq!(q.frame(None, out + MS * 16, CELL, PT), None);
        }
    }

    /// **A fading effect still goes out at its own end, when that comes
    /// first**: a lightning bolt, out in about a third of a second, hidden
    /// 30 ms after it was taken, is out at its end with its fade a fifth
    /// from done.
    #[test]
    fn a_fading_effect_goes_out_at_its_own_end_when_that_is_first() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Effect(Effect::Lightning));
        jumped(&mut q, t0);
        let end = q.fire.expect("struck").end;
        assert!(
            end < 0.36,
            "the control: the bolt ends inside the fade, {end}"
        );
        let ms = |secs: f32| MS * u32::try_from((secs * 1000.0) as i64).unwrap();
        let faded = q.frame(None, t0 + MS * 50, CELL, PT).expect("fading");
        assert!(opacity(&faded) < 1.0, "hidden: faded");
        let before = t0 + MS * 2 + ms(end) - MS * 3;
        assert!(q.frame(None, before, CELL, PT).is_some(), "before its end");
        let after = t0 + MS * 2 + ms(end) + MS * 2;
        assert_eq!(q.frame(None, after, CELL, PT), None, "out at its end");
        assert_eq!(q.owes(after + MS), Owes::Nothing);
    }

    /// **A cursor shown again while its trail fades brings the trail back
    /// at the rate it faded**, as kitty's does
    /// (`[[source: kitty-cursor-trail]]` 134-136): the sweep, hidden from
    /// the frame after 20 ms to 180 ms, is at 0.6 on the last hidden frame
    /// and rises from there, counted from that frame, to whole, the hole on
    /// the cursor again; and a long move after it is followed as any is.
    #[test]
    fn a_cursor_shown_mid_fade_brings_its_trail_back_at_the_same_rate() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Sweep));
        jumped(&mut q, t0);
        let to = at(90.0, 20.0);
        let mut low = None;
        for ms in (36..=180).step_by(16) {
            low = q.frame(None, t0 + MS * ms, CELL, PT);
        }
        let low = low.expect("fading");
        assert!(at_level(opacity(&low), 0.6), "{}", opacity(&low));
        let back = q.frame(Some(to), t0 + MS * 196, CELL, PT).expect("back");
        assert!(at_level(opacity(&back), 0.64), "{}", opacity(&back));
        assert_eq!(hole(&back), Some(to));
        let up = q.frame(Some(to), t0 + MS * 260, CELL, PT).expect("rising");
        assert!(at_level(opacity(&up), 0.8), "{}", opacity(&up));
        let whole = q.frame(Some(to), t0 + MS * 360, CELL, PT).expect("whole");
        assert_eq!(opacity(&whole), 1.0);
        let next = at(10.0, 2.0);
        q.frame(Some(next), t0 + MS * 376, CELL, PT);
        let moved = q.frame(Some(next), t0 + MS * 392, CELL, PT).expect("taken");
        assert_eq!((hole(&moved), opacity(&moved)), (Some(next), 1.0));
    }

    /// **A cursor hidden at rest draws nothing and owes nothing**, and one
    /// shown after a fade that ran out comes back as kitty's does
    /// (`[[source: kitty-cursor-trail]]` 134-136): its next trail starts
    /// from the place it was hidden at, at the level the opacity has risen
    /// to since the last hidden frame, here 624 ms -- 96 ms of rise, 0.24.
    #[test]
    fn a_cursor_hidden_at_rest_draws_nothing() {
        let t0 = Instant::now();
        let mut q = Chase::new(Style::Smear(Shape::Quad));
        q.frame(Some(at(5.0, 5.0)), t0, CELL, PT);
        for k in 1..40u32 {
            let now = t0 + MS * (16 * k);
            assert_eq!(q.frame(None, now, CELL, PT), None, "{k}");
            assert_eq!(q.owes(now), Owes::Nothing, "{k}");
        }
        let far = at(60.0, 5.0);
        q.frame(Some(far), t0 + MS * 700, CELL, PT);
        let look = q
            .frame(Some(far), t0 + MS * 720, CELL, PT)
            .expect("a trail");
        let TrailLook::Smear(s) = look else {
            panic!("{look:?}")
        };
        assert!(s.corners.0[3].x < at(30.0, 5.0).left, "from where it hid");
        assert!(at_level(opacity(&look), 0.24), "{}", opacity(&look));
    }

    /// The flash bound's tests: a cell 42 pt tall, above the gate, unless a
    /// test says otherwise.
    mod bound {
        use super::*;

        const ABOVE: Points = Points(42.0);

        /// The strength a frame's look carries, if it draws one.
        fn strength(look: Option<TrailLook>) -> Option<Strength> {
            match look {
                Some(TrailLook::Vfx(v)) => Some(v.emitter.strength),
                Some(TrailLook::Smear(s)) => Some(s.strength),
                _ => None,
            }
        }

        /// The cursor moved to `to` at `ms` after `t0`, and the frame after
        /// its dwell: the strength that move is drawn at.
        fn moved(
            q: &mut Chase<Instant>,
            t0: Instant,
            to: Rect,
            after: Duration,
            pt: Points,
        ) -> Option<Strength> {
            q.frame(Some(to), t0 + after, CELL, pt);
            strength(q.frame(Some(to), t0 + after + MS * 12, CELL, pt))
        }

        /// **A back-and-forth every 100 ms is full on its first move and a
        /// repeat on every later one inside the second**, the retargets and
        /// the relights alike; the first move a second after the full one is
        /// full again.
        #[test]
        fn a_back_and_forth_is_full_once_a_second() {
            let t0 = Instant::now();
            let (a, b) = (at(10.0, 5.0), at(40.0, 12.0));
            let mut q = Chase::new(Style::Effect(Effect::Fire));
            q.frame(Some(a), t0, CELL, ABOVE);
            assert_eq!(
                moved(&mut q, t0, b, MS * 8, ABOVE),
                Some(Strength::Full),
                "the first move"
            );
            let mut seeds = std::collections::BTreeSet::new();
            for k in 1..10u32 {
                let to = if k % 2 == 1 { a } else { b };
                q.frame(Some(to), t0 + MS * (8 + 100 * k), CELL, ABOVE);
                let look = q.frame(Some(to), t0 + MS * (20 + 100 * k), CELL, ABOVE);
                if let Some(TrailLook::Vfx(v)) = look {
                    seeds.insert(v.ignition.seed.0);
                }
                assert_eq!(
                    strength(look),
                    Some(Strength::UnderThreshold),
                    "move {k}, {} ms after the full one",
                    100 * k
                );
            }
            assert!(seeds.len() >= 2, "the control: relights among the repeats");
            assert_eq!(moved(&mut q, t0, b, MS * 1008, ABOVE), Some(Strength::Full));
        }

        /// **A move whose band misses the ledger is full**, inside the
        /// second; and the control, one over the first stretch again, is not.
        #[test]
        fn a_band_that_misses_the_ledger_is_full() {
            let t0 = Instant::now();
            let mut q = Chase::new(Style::Effect(Effect::Fire));
            q.frame(Some(at(10.0, 5.0)), t0, CELL, ABOVE);
            assert_eq!(
                moved(&mut q, t0, at(40.0, 5.0), MS * 8, ABOVE),
                Some(Strength::Full)
            );
            q.snap();
            q.frame(Some(at(10.0, 30.0)), t0 + MS * 100, CELL, ABOVE);
            assert_eq!(
                moved(&mut q, t0, at(40.0, 40.0), MS * 500, ABOVE),
                Some(Strength::Full),
                "a band 25 rows below the first"
            );
            q.snap();
            q.frame(Some(at(40.0, 5.0)), t0 + MS * 800, CELL, ABOVE);
            assert_eq!(
                moved(&mut q, t0, at(10.0, 5.0), MS * 850, ABOVE),
                Some(Strength::UnderThreshold),
                "the control: back over the first stretch"
            );
        }

        /// **A relight inside the second is a repeat**: a new ignition past
        /// [`REIGNITE`], over the stretch the first one swept.
        #[test]
        fn a_relight_inside_the_second_is_a_repeat() {
            let t0 = Instant::now();
            let (a, b) = (at(10.0, 5.0), at(60.0, 5.0));
            let mut q = Chase::new(Style::Effect(Effect::Fire));
            q.frame(Some(a), t0, CELL, ABOVE);
            assert_eq!(moved(&mut q, t0, b, MS * 8, ABOVE), Some(Strength::Full));
            q.frame(Some(a), t0 + MS * 408, CELL, ABOVE);
            let look = q.frame(Some(a), t0 + MS * 420, CELL, ABOVE);
            let Some(TrailLook::Vfx(v)) = look else {
                panic!("a relight draws: {look:?}");
            };
            assert_eq!(v.ignition.seed, Seed(2), "the control: a new ignition");
            assert_eq!(v.emitter.strength, Strength::UnderThreshold);
        }

        /// **A restyle keeps the ledger**, as it keeps the ignition guard: a
        /// switch to the quad and a move back over the stretch inside the
        /// second is a repeat; a fresh chase draws the same move full.
        #[test]
        fn a_restyle_keeps_the_ledger() {
            let t0 = Instant::now();
            let (a, b) = (at(10.0, 5.0), at(60.0, 5.0));
            let mut q = Chase::new(Style::Effect(Effect::Fire));
            q.frame(Some(a), t0, CELL, ABOVE);
            assert_eq!(moved(&mut q, t0, b, MS * 8, ABOVE), Some(Strength::Full));
            q.restyle(Style::Smear(Shape::Quad));
            q.frame(Some(b), t0 + MS * 100, CELL, ABOVE);
            assert_eq!(
                moved(&mut q, t0, a, MS * 200, ABOVE),
                Some(Strength::UnderThreshold)
            );
            let mut fresh = Chase::new(Style::Smear(Shape::Quad));
            fresh.frame(Some(b), t0 + MS * 100, CELL, ABOVE);
            assert_eq!(
                moved(&mut fresh, t0, a, MS * 200, ABOVE),
                Some(Strength::Full)
            );
        }

        /// **A ledger full of live passes forgets none**: nine disjoint
        /// long moves inside a second, each far from the others, are full
        /// while the ledger has room and the ninth is a repeat rather than
        /// evict a pass that still holds its screen. The control: a second
        /// later the ledger has expired and the same move is full.
        #[test]
        fn a_full_ledger_draws_a_repeat_rather_than_forget() {
            let t0 = Instant::now();
            let mut q = Chase::new(Style::Smear(Shape::Quad));
            let mut seen = Vec::new();
            for k in 0..9u32 {
                let row = 40.0 * f32::from(u16::try_from(k).unwrap());
                q.snap();
                q.frame(Some(at(10.0, row)), t0 + MS * (100 * k), CELL, ABOVE);
                seen.push(moved(&mut q, t0, at(30.0, row), MS * (100 * k + 8), ABOVE));
            }
            assert_eq!(
                seen[..8],
                [Some(Strength::Full); 8],
                "eight passes, each on its own screen"
            );
            assert_eq!(seen[8], Some(Strength::UnderThreshold), "the ninth");
            q.snap();
            q.frame(Some(at(10.0, 400.0)), t0 + MS * 2000, CELL, ABOVE);
            assert_eq!(
                moved(&mut q, t0, at(30.0, 400.0), MS * 2008, ABOVE),
                Some(Strength::Full),
                "the control: the ledger expired"
            );
        }

        /// **The ghost's sweeps are weighed as a long move is**: back and
        /// forth every 100 ms on a cell above the gate, the first sweep is
        /// full and every later one inside the second a repeat over its
        /// band, the first a second after the full one full again; on a
        /// 17 pt cell, and on one at the gate, every sweep is full.
        #[test]
        fn a_ghost_back_and_forth_is_full_once_a_second() {
            let cell = |(r, c): (Row, Col)| {
                let n = |v: usize| f32::from(u16::try_from(v).unwrap());
                at(n(c.index()), n(r.index()))
            };
            let (a, b) = (
                (Row::from_index(5), Col::at_index(10)),
                (Row::from_index(12), Col::at_index(40)),
            );
            for pt in [ABOVE, PT, GATE] {
                let t0 = Instant::now();
                let mut ghosts = Ledger::new();
                let rests = ghost_sweep(&mut ghosts, Some(a), Some(a), t0, cell, (CELL, pt));
                assert_eq!(rests, None, "the control: a cursor at rest sweeps none");
                let mut seen = Vec::new();
                for k in 0..10u32 {
                    let (from, to) = if k % 2 == 0 { (a, b) } else { (b, a) };
                    let t = t0 + MS * (100 * k);
                    let sweep = ghost_sweep(&mut ghosts, Some(from), Some(to), t, cell, (CELL, pt))
                        .expect("a move sweeps");
                    assert_eq!((sweep.from, sweep.to), (from, to));
                    seen.push(sweep.strength);
                }
                let later = t0 + MS * 1008;
                let again = ghost_sweep(&mut ghosts, Some(a), Some(b), later, cell, (CELL, pt))
                    .map(|s| s.strength);
                let repeat = if pt == ABOVE {
                    Strength::UnderThreshold
                } else {
                    Strength::Full
                };
                assert_eq!(seen[0], Strength::Full, "{pt:?}: the first sweep");
                assert_eq!(seen[1..], [repeat; 9], "{pt:?}: the sweeps after it");
                assert_eq!(again, Some(Strength::Full), "{pt:?}: a second later");
            }
        }

        /// **A restyle keeps the ledger the ghost weighs on**: a fire's
        /// full pass above the gate, a restyle, and a ghost's sweep back
        /// over it on [`Chase::ledger`] inside the second is a repeat; the
        /// control, a fresh chase's ledger, is full.
        #[test]
        fn a_restyle_keeps_the_ledger_the_ghost_weighs_on() {
            let t0 = Instant::now();
            let cell = |(r, c): (Row, Col)| {
                let n = |v: usize| f32::from(u16::try_from(v).unwrap());
                at(n(c.index()), n(r.index()))
            };
            let (a, b) = (
                (Row::from_index(5), Col::at_index(10)),
                (Row::from_index(12), Col::at_index(40)),
            );
            let mut q = Chase::new(Style::Effect(Effect::Fire));
            q.frame(Some(cell(a)), t0, CELL, ABOVE);
            let lit = moved(&mut q, t0, cell(b), MS * 8, ABOVE);
            assert_eq!(lit, Some(Strength::Full), "the control: a full fire");
            q.restyle(Style::Smear(Shape::Quad));
            let mut fresh = Chase::<Instant>::new(Style::Smear(Shape::Quad));
            let later = t0 + MS * 300;
            let sweep = |l: &mut Ledger<Instant>| {
                ghost_sweep(l, Some(b), Some(a), later, cell, (CELL, ABOVE)).map(|s| s.strength)
            };
            assert_eq!(
                sweep(fresh.ledger()),
                Some(Strength::Full),
                "a fresh ledger"
            );
            assert_eq!(sweep(q.ledger()), Some(Strength::UnderThreshold));
        }

        /// **At or below the gate every move is full**: the same
        /// back-and-forth on a 17 pt cell, and on a 32 pt one, the gate
        /// itself.
        #[test]
        fn at_or_below_the_gate_everything_is_full() {
            for pt in [PT, GATE] {
                let t0 = Instant::now();
                let (a, b) = (at(10.0, 5.0), at(40.0, 12.0));
                let mut q = Chase::new(Style::Effect(Effect::Fire));
                q.frame(Some(a), t0, CELL, pt);
                for k in 0..10u32 {
                    let to = if k % 2 == 0 { b } else { a };
                    assert_eq!(
                        moved(&mut q, t0, to, MS * (8 + 100 * k), pt),
                        Some(Strength::Full),
                        "{pt:?}, move {k}"
                    );
                }
            }
        }
    }
}
