//! **The Linux driver's GPU arm**: a terminal's frame built as the cell
//! path's (`baud_render::cell_frame::build`), drawn into one of two exported
//! targets ([`Gpu::draw_cells`]) and handed back for `present_dmabuf` -- or
//! refused, so the driver draws it on the CPU arm as before.
//!
//! **What takes the CPU arm**, each counted ([`Cpu`]): a cell pipeline still
//! building, a trail's among them, which is never waited for -- its build
//! runs on the worker and wakes the loop through [`Presenter::ready_fd`];
//! and an image on screen, which the cell path does not draw. A smear, an
//! effect trail and the fireworks are drawn in the cell pass by their own
//! stages. A frame on
//! the CPU arm leaves both targets lacking what it changed
//! ([`Presenter::drawn_elsewhere`]).
//!
//! **A frame with no free target is owed** ([`Frame::Owed`]): both are the
//! compositor's, which is behind, and a CPU frame is the costliest answer.
//! The rows stay owed to the next frame drawn. A target is the compositor's
//! from the frame shown into it until its release -- the `wl_buffer.release`
//! of a frame shown without points ([`Presenter::released`]), or its release
//! point reached for one shown with points, which a descriptor the loop
//! polls says ([`Presenter::release_fd`]).
//!
//! **One frame flies at a time** ([`Frame::Busy`]): where the target has a
//! timeline, the frame's own submission signals its acquire point, nothing
//! waits for it, and its end is a sync file the loop polls
//! ([`Presenter::done_fd`]); a frame asked for meanwhile is owed and drawn
//! once it lands ([`Presenter::land`]). Without points the end is put on the
//! target's dmabuf as its write fence, which the compositor's reads wait on;
//! where the device or the targets take no sync file, or the device is a CPU
//! device, whose export of one returns only once the frame has ended, a
//! frame is finished before it is shown, counted ([`Flies`],
//! [`Counts::waited`]). An end exported as `-1`, a semaphore already
//! signalled, is a frame finished at its export, counted
//! ([`Counts::signalled`]).
//!
//! The driver passes no smooth scroll: no pan and no leaving row
//! ([`Inputs`]).

use crate::cells::{
    CellDraw, CellMemory, CellSizes, CellSlices, End, Finish, Landing, MapSide, Met,
};
use crate::damage::{TARGETS, TargetIx};
use crate::frame::{Failed, Gpu, Target, Timeline};
use crate::lend::{Arm, Elems};
use crate::pipeline::{Blend as PipeBlend, Fragment, Fx, Pipelines, Skipped};
use crate::sheets::Sheets;
use baud_core::{Seal, Term};
use baud_domain::{Codepoint, Col, PackedCell, Row, RowExtent, Rows};
use baud_font::Px as FontPx;
use baud_present_wayland::dmabuf::Point;
use baud_present_wayland::{Changed, Px};
use baud_render::cell_frame::{CellInputs, CellParts, build_with};
use baud_render::screens::{SETS, Screens, SetIx};
use baud_render::{
    Candidate, Composition, GlyphMap, Instance, Kind, Leaving, Lent, Overlay, Redraw, Renderer,
    SelectedSpan, StyleStamp,
};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

/// The cell path's pipelines, every one a frame may draw through.
const CELL_PIPES: [(Fragment, PipeBlend); 5] = [
    (Fragment::CellBg, PipeBlend::Replace),
    (Fragment::CellGlyph, PipeBlend::Normal),
    (Fragment::CellShape, PipeBlend::Normal),
    (Fragment::Glyph, PipeBlend::Normal),
    (Fragment::CellRing, PipeBlend::Normal),
];

/// Why a frame is drawn on the CPU arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cpu {
    /// A cell pipeline is still building.
    Building,
    /// An image is on screen.
    Images,
    /// A trail's records that no stage of this path draws: none of the
    /// smear, the effects and the fireworks.
    Trail,
}

/// What [`Presenter::frame`] did with a frame.
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    /// Drawn into `target`, for `present_dmabuf` with
    /// [`Presenter::damage`] and [`Presenter::points`].
    Drew(TargetIx),
    /// No target is free: nothing drawn, the rows owed.
    Owed,
    /// The frame before is still in flight: nothing drawn, the rows owed,
    /// and the frame drawn when it lands ([`Presenter::land`]).
    Busy,
    /// The CPU arm draws it.
    Cpu(Cpu),
}

/// How often each arm was taken.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub drew: Skipped,
    pub owed: Skipped,
    pub busy: Skipped,
    pub building: Skipped,
    pub images: Skipped,
    pub trail: Skipped,
    /// Frames drawn and finished on the CPU before they returned
    /// ([`Flies::Waits`], or an end the device or the target refused for
    /// that frame).
    pub waited: Skipped,
    /// Frames whose end exported as `-1`, the answer for a semaphore
    /// already signalled: finished by the time the export returned, met
    /// there rather than left in flight or waited for.
    pub signalled: Skipped,
    /// Frames owed to a release point with nothing to wake the loop when
    /// it is reached: no wake was made ([`crate::sys::wake_at`] answered
    /// none), so the frame waits for the next thing that wakes the loop.
    pub unwoken: Skipped,
}

/// **Whether a frame drawn is left in flight**, answered once per target
/// set, or finished on the CPU before it is shown, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flies {
    Yes,
    Waits(Why),
}

/// Why a frame is finished on the CPU before it is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The device hands out no sync file for a frame's end.
    NoSyncFile,
    /// The device's export of a frame's end returns only once the frame
    /// has ended ([`crate::vk::SyncExport::WaitsForFrame`]): the frame is
    /// finished by the time it could fly.
    ExportWaits,
    /// Without points, the targets' dmabufs take no sync file
    /// ([`crate::sys::takes_sync_files`]), so nothing would make the
    /// compositor's reads wait for the frame.
    FenceRefused,
}

impl Flies {
    /// **Whether frames on `gpu` fly**, `targets` the dmabuf answer where
    /// frames carry no points: explicit sync needs only the device's sync
    /// file, implicit sync its targets' too, and neither flies where the
    /// device's export of that sync file waits for the frame. Targets that
    /// refuse a sync file are named before such an export: they keep a
    /// frame from flying on any device.
    fn of(gpu: &Gpu, targets: crate::sys::SyncFiles) -> Flies {
        use crate::choose::Sync;
        use crate::sys::SyncFiles;
        use crate::vk::{Has, SyncExport};
        match (gpu.sync_fd, gpu.choice.sync, targets, gpu.sync_export) {
            (Has::No, _, _, _) => Flies::Waits(Why::NoSyncFile),
            (_, Sync::Implicit, SyncFiles::Refused, _) => Flies::Waits(Why::FenceRefused),
            (_, _, _, SyncExport::WaitsForFrame) => Flies::Waits(Why::ExportWaits),
            (_, Sync::Explicit, _, SyncExport::Pending)
            | (_, Sync::Implicit, SyncFiles::Taken, SyncExport::Pending) => Flies::Yes,
        }
    }
}

/// **Whether a frame was asked for while the one before flew**, so its
/// landing owes the loop a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owes {
    Frame,
    Nothing,
}

/// **What a frame is built from beyond the terminal and the renderer**: the
/// view's look back, the selection, the composition and the predicted
/// echoes. No pan and no leaving row: the Linux driver draws none.
pub struct Inputs<'i> {
    pub back: Rows,
    pub highlight: Option<SelectedSpan>,
    pub composition: &'i Option<Composition>,
    pub predictions: &'i [(Row, Col, Codepoint)],
}

/// Who holds a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Holder {
    /// Free to draw into.
    Us,
    /// Drawn and not yet shown.
    Drawn,
    /// Shown without points, until its release.
    Compositor,
    /// Shown with points, until the release point.
    Until(Point),
}

/// One of the two targets, and its timeline where frames are synchronised
/// explicitly.
struct Slot {
    target: Target,
    timeline: Option<Timeline>,
    holder: Holder,
    /// The last point signalled or waited on this target's timeline.
    point: Point,
    /// The acquire point the frame drawn into it signals, and the release
    /// point after it, until the frame is shown ([`Presenter::points`]).
    points: Option<(Point, Point)>,
}

/// **The GPU arm's state**: the cell path's memory and its frame's parts,
/// as the macOS app holds them, and the two targets.
pub struct Presenter {
    pipes: Pipelines,
    mem: CellMemory,
    sheets: Sheets,
    lent: CellSlices,
    map: GlyphMap<'static>,
    exceptions: Overlay,
    screens: Screens,
    stamps: [StyleStamp; SETS],
    lending: [Lent; SETS],
    candidates: Vec<Candidate>,
    records: Vec<Instance>,
    grounds: Vec<Instance>,
    lent_used: Vec<RowExtent>,
    used: Vec<RowExtent>,
    slots: Vec<Slot>,
    next: TargetIx,
    counts: Counts,
    /// What the last frame landed cost: the CPU's part, from its target in
    /// hand to its submission, and the GPU's pass (`timing.rs`), or with a
    /// wait from target to fence. What effect pacing weighs.
    cost: std::time::Duration,
    /// Whether the last frame landed's build drew into the atlas.
    cold: baud_render::Cold,
    /// The CPU's part of the frame in flight.
    cpu_cost: std::time::Duration,
    /// Whether the frame in flight's build drew into the atlas.
    cpu_cold: baud_render::Cold,
    /// Whether a frame was asked for while the one in flight flew.
    owes: Owes,
    /// Whether a frame drawn flies, answered when the targets are made.
    flies: Flies,
    /// Readable once a release point a frame is owed to is reached.
    release: Option<OwnedFd>,
}

impl Presenter {
    /// **The arm for a terminal like `term`**, its memory on the arm the
    /// device's heaps allow, and no target yet ([`Presenter::fit`]).
    pub fn new(gpu: &Gpu, term: &Term<'_>) -> Result<Presenter, Failed> {
        let pipes = Pipelines::new(gpu)?;
        // **Every set holds the largest grid the storage is provisioned
        // for**: as many rows as a resize can grow to
        // (`Term::row_capacity`), each the pitch wide, which no width can
        // pass. A window that grows after the arm starts draws into the
        // memory it has, the status line's row included.
        let rows = term.row_capacity();
        let cols = baud_domain::Cols::of_index(term.stride().index());
        let sizes = CellSizes {
            cols,
            rows,
            stride: term.stride(),
            styles: Elems(baud_render::cell_styles_len(term, cols, rows).0),
        };
        // The copies, the larger part of what the sets hold.
        let need =
            term.stride().index() * (rows.index() + 1) * core::mem::size_of::<PackedCell>() * SETS;
        let arm = gpu.arm(u64::try_from(need).unwrap_or(u64::MAX));
        // A failure part way destroys what the steps before it made: the
        // memory when the glyph map or the sheets fail, the pipelines when
        // anything after them does.
        let made = CellMemory::new(gpu, &pipes, arm, sizes).and_then(|(mem, mut lent)| {
            let held = lent.maps[0]
                .take()
                .ok_or(Failed::Outside)
                .and_then(|m| Ok((m, gpu.sheets(&baud_domain::contrast::CURVES[..])?)));
            match held {
                Ok((m, sheets)) => Ok((mem, lent, sheets, GlyphMap::over(m))),
                Err(e) => {
                    drop(lent);
                    mem.destroy(gpu);
                    Err(e)
                }
            }
        });
        let (mem, lent, sheets, map) = match made {
            Ok(made) => made,
            Err(e) => {
                pipes.destroy();
                return Err(e);
            }
        };
        Ok(Presenter {
            pipes,
            mem,
            sheets,
            lent,
            map,
            exceptions: Overlay::new(),
            screens: Screens::ringed(),
            stamps: [StyleStamp::default(); SETS],
            lending: core::array::from_fn(|_| Lent::default()),
            candidates: Vec::new(),
            records: Vec::new(),
            grounds: Vec::new(),
            lent_used: Vec::new(),
            used: Vec::new(),
            slots: Vec::new(),
            next: TargetIx(0),
            counts: Counts::default(),
            cost: std::time::Duration::ZERO,
            cold: baud_render::Cold::No,
            cpu_cost: std::time::Duration::ZERO,
            cpu_cold: baud_render::Cold::No,
            owes: Owes::Nothing,
            // Until the targets answer, a frame without points waits.
            flies: Flies::of(gpu, crate::sys::SyncFiles::Refused),
            release: None,
        })
    }

    /// **Two targets of `size`**, made anew when they are not that size:
    /// whether they were, so the driver imports each ([`Presenter::target`]).
    /// A new target holds no frame and draws its first whole.
    pub fn fit(&mut self, gpu: &Gpu, size: (Px, Px)) -> Result<Remade, Failed> {
        if self.slots.len() == TARGETS
            && self
                .slots
                .iter()
                .all(|s| (s.target.width, s.target.height) == size)
        {
            return Ok(Remade::No);
        }
        // A target the frame in flight draws into stays until it lands.
        if self.land(gpu)? == Landing::Flying {
            self.counts.busy.0 += 1;
            self.owes = Owes::Frame;
            return Ok(Remade::Busy);
        }
        self.drop_slots(gpu);
        // Both targets or none: a failure part way drops what was made.
        if let Err(e) = self.make_slots(gpu, size) {
            self.drop_slots(gpu);
            return Err(e);
        }
        self.next = TargetIx(0);
        Ok(Remade::Yes)
    }

    /// Both targets made, each with its timeline where frames carry points;
    /// a failure part way leaves the ones made in `slots`.
    fn make_slots(&mut self, gpu: &Gpu, size: (Px, Px)) -> Result<(), Failed> {
        for _ in 0..TARGETS {
            let target = gpu.target(size.0, size.1)?;
            let timeline = match gpu.choice.sync {
                crate::choose::Sync::Explicit => match gpu.timeline() {
                    Ok(tl) => Some(tl),
                    Err(e) => {
                        gpu.drop_target(target);
                        return Err(e);
                    }
                },
                crate::choose::Sync::Implicit => None,
            };
            self.slots.push(Slot {
                target,
                timeline,
                holder: Holder::Us,
                point: Point(0),
                points: None,
            });
        }
        // Asked of one target: both are made alike, on one device.
        let takes = match (gpu.choice.sync, self.slots.first()) {
            (crate::choose::Sync::Implicit, Some(s)) => {
                crate::sys::takes_sync_files(s.target.plane().fd)
            }
            _ => crate::sys::SyncFiles::Refused,
        };
        #[cfg(feature = "test-hooks")]
        let takes = gpu.hooks.sync_files(takes);
        self.flies = Flies::of(gpu, takes);
        Ok(())
    }

    /// Target `k`, for the driver to import, and its timeline's fd.
    pub fn target(&self, k: TargetIx) -> Option<(&Target, Option<&Timeline>)> {
        self.slots
            .get(k.0)
            .map(|s| (&s.target, s.timeline.as_ref()))
    }

    /// **The frame drawn into target `k`**, built from `term` through
    /// `renderer` under `seal`: the CPU arm's where a pipeline builds or the
    /// frame shows what the cell path does not draw, owed where no target
    /// is free, else drawn.
    pub fn frame(
        &mut self,
        gpu: &Gpu,
        renderer: &mut Renderer<'_, '_>,
        term: &Term<'_>,
        seal: Seal,
        inputs: &Inputs<'_>,
    ) -> Result<Frame, Failed> {
        // One frame flies at a time: nothing the one in flight reads is
        // written, and asked meanwhile, a frame is owed and never waited for.
        if self.land(gpu)? == Landing::Flying {
            self.counts.busy.0 += 1;
            self.owes = Owes::Frame;
            return Ok(Frame::Busy);
        }
        let building = CELL_PIPES
            .iter()
            .filter(|(f, b)| self.pipes.ready(*f, *b).is_none())
            .count();
        if building > 0 {
            self.counts.building.0 += 1;
            return Ok(Frame::Cpu(Cpu::Building));
        }
        let Some(k) = self.free(gpu)? else {
            self.counts.owed.0 += 1;
            self.wake_at_release(gpu);
            return Ok(Frame::Owed);
        };
        // The frame's cost runs from here, a target in hand, to its fence.
        let started = std::time::Instant::now();
        renderer.fit(term);
        renderer.set_highlight(inputs.highlight);
        renderer.set_composition(inputs.composition.clone());
        renderer.set_images(term, inputs.back);
        if !renderer.image_placements().is_empty() {
            self.counts.images.0 += 1;
            return Ok(Frame::Cpu(Cpu::Images));
        }
        // A smear, an effect and the fireworks are drawn in the cell pass by
        // their own stages, which a frame never waits for: while one builds,
        // the frame is the CPU arm's.
        let trail = renderer
            .trail()
            .iter()
            .any(|r| matches!(r.kind, Kind::Quad | Kind::Band | Kind::Particle));
        if trail {
            let needs: Vec<(Fragment, PipeBlend)> = match (
                renderer.fireworks_uniform([0.0; 2]),
                renderer.smear(),
                renderer.vfx(),
            ) {
                (Some(_), _, _) => vec![(Fragment::Fireworks, PipeBlend::Normal)],
                (None, Some(_), _) => vec![(Fragment::Quad, PipeBlend::Normal)],
                (None, None, Some(v)) => {
                    let (fx, blend) = Fx::mixed(v.effect, renderer.trail_mix());
                    gpu.passes(fx, blend)
                        .into_iter()
                        .map(|(fx, b)| (Fragment::Effect(fx), b))
                        .collect()
                }
                (None, None, None) => {
                    self.counts.trail.0 += 1;
                    return Ok(Frame::Cpu(Cpu::Trail));
                }
            };
            let building = needs
                .iter()
                .filter(|(f, b)| self.pipes.ready(*f, *b).is_none())
                .count();
            if building > 0 {
                self.counts.building.0 += 1;
                return Ok(Frame::Cpu(Cpu::Building));
            }
        }
        renderer.place_lights(term);
        let live_screen = term.looking_back(inputs.back).index() == 0;
        let style = term.cursor_style();
        let status = term
            .status_line()
            .and_then(|l| l.cursor)
            .map(|c| (Row::from_index(term.geometry().1.index()), c, style));
        let live = status.filter(|_| live_screen).or_else(|| {
            (term.cursor_visible() && live_screen).then(|| {
                let (row, col) = term.cursor();
                (row, col, style)
            })
        });
        let cell_inputs = CellInputs {
            back: inputs.back,
            predictions: inputs.predictions.iter().copied(),
            live,
            composition: inputs.composition,
            highlight: renderer.highlight(),
            leaving: Leaving::None,
            pan: FontPx::of(0),
        };
        let damage = renderer.damage(term, inputs.back);
        let Some(frame) = build_with(
            renderer,
            term,
            seal,
            &damage,
            &cell_inputs,
            &mut CellParts {
                map: &mut self.map,
                exceptions: &mut self.exceptions,
                screens: &mut self.screens,
                copies_mem: &mut self.lent.copies,
                styles_mem: &mut self.lent.styles,
                overlay_mem: &mut self.lent.overlay,
                style_stamps: &mut self.stamps,
                lent: &mut self.lending,
                candidates: &mut self.candidates,
                records: &mut self.records,
                grounds: &mut self.grounds,
                lent_used: &mut self.lent_used,
            },
            &mut self.used,
            || Some(SetIx(0)),
        ) else {
            return Err(Failed::Outside);
        };
        let input = CellDraw {
            set: frame.at,
            stride: term.stride(),
            rotation: frame.rot,
            ring: frame.ring,
            cursor: frame.cursor,
            cursor_drawn: self.exceptions.cursor_drawn(),
            highlight: renderer.highlight(),
            records: &self.records,
            grounds: &self.grounds,
            under: self.exceptions.under(),
            chrome: frame.chrome,
            slide: None,
            table: self.stamps[frame.at.0],
            reach: self.map.reach(),
            map: (self.map.entries(), MapSide::First),
            screen: self.screens.now(),
            overlay: &self.lent.overlay[frame.at.0][..],
            cells: &self.lent.copies[frame.at.0][..],
            styles: &self.lent.styles[frame.at.0][..],
            moving: (Elems(frame.moving.0), Elems(frame.moving.1)),
            map_gen: self.map.generation(),
        };
        let slot = &mut self.slots[k.0];
        // The acquire point is the frame's own submission's to signal, so
        // the compositor reads the target once the GPU has drawn it; the
        // release point is the one after.
        let points = slot
            .timeline
            .as_ref()
            .map(|tl| (tl, Point(slot.point.0 + 1)));
        // In flight where the compositor waits on the GPU itself: at the
        // acquire point, or without points on the target's write fence the
        // frame's end is put on (`Gpu::sync_file`); else finished before it
        // is shown.
        let finish = Finish {
            end: match self.flies {
                Flies::Yes => End::Fly,
                Flies::Waits(_) => End::Wait,
            },
            acquire: points,
        };
        let drawn = gpu.draw_cells(
            &mut self.pipes,
            &mut self.mem,
            &mut self.sheets,
            renderer,
            &input,
            &slot.target,
            finish,
        )?;
        if let Some((_, acquire)) = points {
            slot.point = Point(acquire.0 + 1);
            slot.points = Some((acquire, slot.point));
        }
        slot.holder = Holder::Drawn;
        self.cpu_cost = started.elapsed();
        self.cpu_cold = frame.cold;
        // A frame met before `draw_cells` returned costs what it took, and
        // is counted as waited for, or as finished by its export.
        match drawn.met {
            Met::Flying => {}
            Met::Waited => {
                (self.cost, self.cold) = (self.cpu_cost, self.cpu_cold);
                self.counts.waited.0 += 1;
            }
            Met::Signalled => {
                (self.cost, self.cold) = (self.cpu_cost, self.cpu_cold);
                self.counts.signalled.0 += 1;
            }
        }
        self.next = TargetIx((k.0 + 1) % TARGETS);
        self.counts.drew.0 += 1;
        // The frame a release point was owed is drawn.
        self.release = None;
        Ok(Frame::Drew(k))
    }

    /// **The screen as it stands made ready to draw**, off `term`'s window
    /// `back` lines up under `seal` (`cell_frame::prepare`): the atlas, the
    /// shaper's caches and the map warmed, no set written. What the driver
    /// calls on a map and a resize, before the first frame is asked for.
    pub fn prepare(
        &mut self,
        renderer: &mut Renderer<'_, '_>,
        term: &Term<'_>,
        seal: Seal,
        back: Rows,
    ) {
        baud_render::cell_frame::prepare(
            renderer,
            term,
            seal,
            back,
            &mut CellParts {
                map: &mut self.map,
                exceptions: &mut self.exceptions,
                screens: &mut self.screens,
                copies_mem: &mut self.lent.copies,
                styles_mem: &mut self.lent.styles,
                overlay_mem: &mut self.lent.overlay,
                style_stamps: &mut self.stamps,
                lent: &mut self.lending,
                candidates: &mut self.candidates,
                records: &mut self.records,
                grounds: &mut self.grounds,
                lent_used: &mut self.lent_used,
            },
        );
    }

    /// **A wake for the frame owed**, where every target waits for its
    /// release point: one descriptor the kernel signals when any of them is
    /// reached ([`crate::sys::wake_at`]), registered once per owed frame. A
    /// target shown without points is freed by its `wl_buffer.release`,
    /// which wakes the loop itself. Refused, the frame waits for the next
    /// thing that wakes the loop, counted.
    fn wake_at_release(&mut self, gpu: &Gpu) {
        if self.release.is_some() {
            return;
        }
        let points: Vec<(BorrowedFd<'_>, Point)> = self
            .slots
            .iter()
            .filter_map(|s| match (s.holder, &s.timeline) {
                (Holder::Until(p), Some(tl)) => Some((tl.fd.as_fd(), p)),
                _ => None,
            })
            .collect();
        if points.is_empty() {
            return;
        }
        self.release = gpu
            .render_node()
            .and_then(|node| crate::sys::wake_at(node, &points));
        #[cfg(feature = "test-hooks")]
        if gpu.hooks.wakes() == crate::hooks::Wakes::Refused {
            self.release = None;
        }
        if self.release.is_none() {
            self.counts.unwoken.0 += 1;
        }
    }

    /// The descriptor a release point the frame owed waits for makes
    /// readable, for the loop to poll beside [`Presenter::done_fd`].
    pub fn release_fd(&self) -> Option<BorrowedFd<'_>> {
        self.release.as_ref().map(AsFd::as_fd)
    }

    /// **A release point the owed frame waited for is reached**, as
    /// [`Presenter::release_fd`] turned readable: the wake given up, and
    /// the frame owed.
    pub fn release_reached(&mut self) -> Owes {
        match self.release.take() {
            Some(_) => Owes::Frame,
            None => Owes::Nothing,
        }
    }

    /// **The frame in flight, met if it has finished** (`CellMemory::land`):
    /// its cost then the CPU's part and the GPU's pass. What the loop calls
    /// when [`Presenter::done_fd`] turns readable, and each frame first.
    pub fn land(&mut self, gpu: &Gpu) -> Result<Landing, Failed> {
        let landing = self.mem.land(gpu, &self.sheets)?;
        if landing == Landing::Landed {
            self.cost = self.cpu_cost + self.mem.times.pass;
            self.cold = self.cpu_cold;
        }
        Ok(landing)
    }

    /// **Whether a frame asked for while the one before flew is owed**, read
    /// once the frame in flight has landed, and cleared.
    pub fn owed_on_landing(&mut self) -> Owes {
        core::mem::replace(&mut self.owes, Owes::Nothing)
    }

    /// **Whether a frame drawn flies**, as the targets made last answered.
    pub fn flies(&self) -> Flies {
        self.flies
    }

    /// The frame in flight's end, readable once it has finished: for the
    /// loop to poll beside the pipeline builds' ([`Presenter::ready_fd`]).
    pub fn done_fd(&self) -> Option<BorrowedFd<'_>> {
        self.mem.done_fd()
    }

    /// **A free target**, the one after the last drawn first: one the
    /// compositor released, or whose release point is reached.
    fn free(&mut self, gpu: &Gpu) -> Result<Option<TargetIx>, Failed> {
        for i in 0..self.slots.len() {
            let k = (self.next.0 + i) % self.slots.len();
            let s = &mut self.slots[k];
            if let (Holder::Until(p), Some(tl)) = (s.holder, s.timeline.as_ref()) {
                if gpu.reached(tl)?.0 >= p.0 {
                    s.holder = Holder::Us;
                }
            }
            if matches!(s.holder, Holder::Us | Holder::Drawn) {
                return Ok(Some(TargetIx(k)));
            }
        }
        Ok(None)
    }

    /// **The points target `k`'s frame is shown with**, where frames are
    /// synchronised explicitly: its acquire point, which the frame's own
    /// submission signals once the GPU has drawn it, and its release point
    /// the one after. Taken once, by the present that shows it.
    pub fn points(&mut self, k: TargetIx) -> Option<(Point, Point)> {
        self.slots.get_mut(k.0)?.points.take()
    }

    /// **The rows the frame drawn last changed**, for `present_dmabuf`.
    pub fn damage(&self) -> Changed<'_> {
        self.mem.damage()
    }

    /// **The compositor took target `k`'s frame, or did not**: taken, the
    /// target is its until released; refused, its rows stay owed.
    pub fn shown(&mut self, k: TargetIx, shown: baud_present_wayland::DmabufShown) {
        let Some(s) = self.slots.get_mut(k.0) else {
            return;
        };
        match shown {
            baud_present_wayland::DmabufShown::Yes => {
                s.holder = match s.timeline {
                    Some(_) => Holder::Until(s.point),
                    None => Holder::Compositor,
                };
            }
            _ => {
                s.holder = Holder::Us;
                self.mem.refused(&s.target);
            }
        }
    }

    /// Target `k`'s buffer released: free to draw into again.
    pub fn released(&mut self, k: TargetIx) {
        if let Some(s) = self.slots.get_mut(k.0) {
            if s.holder == Holder::Compositor {
                s.holder = Holder::Us;
            }
        }
    }

    /// **A frame drawn on the CPU arm**: neither target holds it, so the
    /// next frame into each draws whole.
    pub fn drawn_elsewhere(&mut self) {
        self.mem.drawn_elsewhere(Redraw::Whole);
    }

    /// The descriptor a finished pipeline build makes readable, for the
    /// loop's wait.
    pub fn ready_fd(&self) -> Option<BorrowedFd<'_>> {
        self.pipes.ready_fd()
    }

    /// **Whether a build finished since the last call**: a frame owed.
    pub fn finished(&mut self) -> crate::pipeline::Built {
        self.pipes.finished()
    }

    /// **What the last frame drawn cost the GPU** (`timing.rs`): its pass and,
    /// where it drew a trail, the trail's draws and their fragments.
    pub fn gpu_times(&self) -> crate::timing::GpuTimes {
        self.mem.times
    }

    /// **What the last frame landed cost**: the CPU's part and the GPU's
    /// pass, or with a wait from its target in hand to its fence.
    pub fn cost(&self) -> std::time::Duration {
        self.cost
    }

    /// **Whether the last frame landed's build drew into the atlas**: a
    /// cold build, whose cost is the atlas's.
    pub fn cold(&self) -> baud_render::Cold {
        self.cold
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// **The cell uniform the frame drawn last was drawn with**.
    pub fn uniforms(&self) -> crate::cells::Cu {
        self.mem.uniforms(SetIx(0))
    }

    /// **The memory types behind the lent slices its frames' builds read
    /// back** ([`CellMemory::read_back`]).
    #[cfg(feature = "test-hooks")]
    pub fn read_back(
        &self,
        gpu: &Gpu,
    ) -> Vec<(&'static str, Option<ash::vk::MemoryPropertyFlags>)> {
        self.mem.read_back(gpu)
    }

    /// The pipelines, for a test to hold their builds.
    pub fn pipes(&self) -> &Pipelines {
        &self.pipes
    }

    /// **The arm its cell memory was made on**, from the device's heaps at
    /// the size of its terminal's copies.
    pub fn arm(&self) -> Arm {
        self.mem.arm()
    }

    fn drop_slots(&mut self, gpu: &Gpu) {
        self.release = None;
        for s in self.slots.drain(..) {
            self.mem.forget(gpu, &s.target);
            if let Some(tl) = s.timeline {
                gpu.drop_timeline(tl);
            }
            gpu.drop_target(s.target);
        }
    }

    /// Everything destroyed, the frame in flight met first and the device
    /// idle.
    pub fn destroy(mut self, gpu: &Gpu) {
        // Teardown waits for the frame in flight: it is off the frame path.
        let _ = self.mem.settle(gpu, &self.sheets);
        self.drop_slots(gpu);
        self.mem.destroy(gpu);
        self.sheets.destroy(gpu);
        self.pipes.destroy();
    }
}

/// Whether [`Presenter::fit`] made the targets anew.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remade {
    Yes,
    No,
    /// Not remade yet: the frame in flight draws into one of them, and a
    /// frame is owed when it lands.
    Busy,
}
