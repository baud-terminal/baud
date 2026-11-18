//! **The Linux driver's GPU arm**: a terminal's frame built as the cell
//! path's (`baud_render::cell_frame::build`), drawn into one of two exported
//! targets ([`Gpu::draw_cells`]) and handed back for `present_dmabuf` -- or
//! refused, so the driver draws it on the CPU arm as before.
//!
//! **What takes the CPU arm**, each counted ([`Cpu`]): a cell pipeline still
//! building, an effect's or the fireworks' among them, which is never waited
//! for -- its build runs on the worker and wakes the loop through
//! [`Presenter::ready_fd`]; an image on screen; and a smear trail's quad,
//! neither of which the cell path draws. An effect trail and the fireworks
//! are drawn in the cell pass by their own stages. A frame on
//! the CPU arm leaves both targets lacking what it changed
//! ([`Presenter::drawn_elsewhere`]).
//!
//! **A frame with no free target is owed** ([`Frame::Owed`]): both are the
//! compositor's, which is behind, and a CPU frame is the costliest answer.
//! The rows stay owed to the next frame drawn. A target is the compositor's
//! from the frame shown into it until its release -- the `wl_buffer.release`
//! of a frame shown without points ([`Presenter::released`]), or its release
//! point reached for one shown with points.
//!
//! The driver passes no smooth scroll: no pan and no leaving row
//! ([`Inputs`]).

use crate::cells::{CellDraw, CellMemory, CellSizes, CellSlices, MapSide};
use crate::damage::{TARGETS, TargetIx};
use crate::frame::{Failed, Gpu, Target, Timeline};
use crate::lend::Elems;
use crate::pipeline::{Blend as PipeBlend, Fragment, Fx, Pipelines, Skipped};
use crate::sheets::Sheets;
use baud_core::{Seal, Term};
use baud_domain::{Codepoint, Col, PackedCell, Row, RowExtent, Rows};
use baud_font::Px as FontPx;
use baud_present_wayland::dmabuf::Point;
use baud_present_wayland::{Changed, Px};
use baud_render::cell_frame::{CellInputs, CellParts, build};
use baud_render::screens::{SETS, Screens, SetIx};
use baud_render::{
    Candidate, Composition, GlyphMap, Highlight, Instance, Kind, Leaving, Lent, Overlay, Redraw,
    Renderer, StyleStamp,
};
use std::os::fd::BorrowedFd;

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
    /// A smear is drawn: the quad has no stage on this path.
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
    /// The CPU arm draws it.
    Cpu(Cpu),
}

/// How often each arm was taken.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub drew: Skipped,
    pub owed: Skipped,
    pub building: Skipped,
    pub images: Skipped,
    pub trail: Skipped,
}

/// **What a frame is built from beyond the terminal and the renderer**: the
/// view's look back, the selection, the composition and the predicted
/// echoes. No pan and no leaving row: the Linux driver draws none.
pub struct Inputs<'i> {
    pub back: Rows,
    pub highlight: Option<Highlight>,
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
    lent_cells: Vec<PackedCell>,
    lent_used: Vec<RowExtent>,
    used: Vec<RowExtent>,
    slots: Vec<Slot>,
    next: TargetIx,
    counts: Counts,
}

impl Presenter {
    /// **The arm for a terminal like `term`**, its memory on the arm the
    /// device's heaps allow, and no target yet ([`Presenter::fit`]).
    pub fn new(gpu: &Gpu, term: &Term<'_>) -> Result<Presenter, Failed> {
        let pipes = Pipelines::new(&gpu.device)?;
        let (cols, rows) = term.geometry();
        let sizes = CellSizes {
            cols,
            rows,
            stride: term.stride(),
            styles: Elems(term.styles().one().len()),
        };
        // The copies, the larger part of what the sets hold.
        let need =
            term.stride().index() * (rows.index() + 1) * core::mem::size_of::<PackedCell>() * SETS;
        let arm = gpu.arm(u64::try_from(need).unwrap_or(u64::MAX));
        let (mem, mut lent) = CellMemory::new(gpu, &pipes, arm, sizes)?;
        let sheets = gpu.sheets(&baud_domain::contrast::CURVES[..])?;
        let map = GlyphMap::over(lent.maps[0].take().ok_or(Failed::Outside)?);
        Ok(Presenter {
            pipes,
            mem,
            sheets,
            lent,
            map,
            exceptions: Overlay::new(),
            screens: Screens::new(),
            stamps: [StyleStamp::default(); SETS],
            lending: core::array::from_fn(|_| Lent::default()),
            candidates: Vec::new(),
            records: Vec::new(),
            grounds: Vec::new(),
            lent_cells: Vec::new(),
            lent_used: Vec::new(),
            used: Vec::new(),
            slots: Vec::new(),
            next: TargetIx(0),
            counts: Counts::default(),
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
        self.drop_slots(gpu);
        for _ in 0..TARGETS {
            let target = gpu.target(size.0, size.1)?;
            let timeline = match gpu.choice.sync {
                crate::choose::Sync::Explicit => Some(gpu.timeline()?),
                crate::choose::Sync::Implicit => None,
            };
            self.slots.push(Slot {
                target,
                timeline,
                holder: Holder::Us,
                point: Point(0),
            });
        }
        self.next = TargetIx(0);
        Ok(Remade::Yes)
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
            return Ok(Frame::Owed);
        };
        renderer.fit(term);
        renderer.set_highlight(inputs.highlight);
        renderer.set_composition(inputs.composition.clone());
        renderer.set_images(term, inputs.back);
        if !renderer.image_placements().is_empty() {
            self.counts.images.0 += 1;
            return Ok(Frame::Cpu(Cpu::Images));
        }
        // An effect and the fireworks are drawn in the cell pass by their
        // own stages, which a frame never waits for: while one builds, the
        // frame is the CPU arm's. A smear's quad has no stage here.
        let trail = renderer
            .trail()
            .iter()
            .any(|r| matches!(r.kind, Kind::Quad | Kind::Band | Kind::Particle));
        if trail {
            let needs: Vec<(Fragment, PipeBlend)> =
                match (renderer.fireworks_uniform([0.0; 2]), renderer.vfx()) {
                    (Some(_), _) => vec![(Fragment::Fireworks, PipeBlend::Normal)],
                    (None, Some(v)) => {
                        let (fx, blend) = Fx::mixed(v.effect, renderer.trail_mix());
                        gpu.passes(fx, blend)
                            .into_iter()
                            .map(|(fx, b)| (Fragment::Effect(fx), b))
                            .collect()
                    }
                    (None, None) => {
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
            highlight: inputs.highlight,
            leaving: Leaving::None,
            pan: FontPx::of(0),
        };
        let Some(frame) = build(
            renderer,
            term,
            seal,
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
                lent_cells: &mut self.lent_cells,
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
            rotation: baud_domain::RowRot::NONE,
            ring: frame.ring,
            cursor: frame.cursor,
            cursor_drawn: self.exceptions.cursor_drawn(),
            highlight: inputs.highlight,
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
            moving: (Elems(frame.moving.0), Elems(frame.moving.1)),
            map_gen: self.map.generation(),
        };
        let slot = &mut self.slots[k.0];
        gpu.draw_cells(
            &mut self.pipes,
            &mut self.mem,
            &mut self.sheets,
            renderer,
            &input,
            &slot.target,
        )?;
        slot.holder = Holder::Drawn;
        self.next = TargetIx((k.0 + 1) % TARGETS);
        self.counts.drew.0 += 1;
        Ok(Frame::Drew(k))
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
    /// synchronised explicitly: its acquire point signalled now -- the
    /// frame is finished, waited for on the CPU -- and its release point
    /// the one after.
    pub fn points(&mut self, gpu: &Gpu, k: TargetIx) -> Result<Option<(Point, Point)>, Failed> {
        let Some(s) = self.slots.get_mut(k.0) else {
            return Ok(None);
        };
        let Some(tl) = s.timeline.as_ref() else {
            return Ok(None);
        };
        let acquire = Point(s.point.0 + 1);
        gpu.signal(tl, acquire)?;
        s.point = Point(acquire.0 + 1);
        Ok(Some((acquire, s.point)))
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

    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// **The cell uniform the frame drawn last was drawn with**.
    pub fn uniforms(&self) -> crate::cells::Cu {
        self.mem.uniforms(SetIx(0))
    }

    /// The pipelines, for a test to hold their builds.
    pub fn pipes(&self) -> &Pipelines {
        &self.pipes
    }

    fn drop_slots(&mut self, gpu: &Gpu) {
        for s in self.slots.drain(..) {
            self.mem.forget(gpu, &s.target);
            if let Some(tl) = s.timeline {
                gpu.drop_timeline(tl);
            }
            gpu.drop_target(s.target);
        }
    }

    /// Everything destroyed, the device idle.
    pub fn destroy(mut self, gpu: &Gpu) {
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
}
