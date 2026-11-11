//! macOS presentation: the window is `macos-liquid-glass`'s; this crate
//! supplies what goes inside `set_content_view`.
//!
//! Two macOS choices are made: one IOSurface-backed texture that the CPU writes
//! under `IOSurfaceLock` or the GPU draws into, presented by setting a plain
//! `CALayer`'s contents -- the lower-level path, no drawable pool -- and a
//! non-opaque layer, because an opaque one paints the glass out (the crate's
//! own luma measurements, pinned as `mlg-measurements`).
//!
//! **Three surfaces in rotation, and nobody waits.** A surface being shown
//! must not be written, so a frame goes to the next one. On the CPU path
//! the frame is painted into the renderer's persistent buffer -- which is
//! what row-granular damage needs, a target that still holds the last
//! frame -- and copied whole into the surface, premultiplied on the way
//! because Core Animation reads `IOSurface` pixels premultiplied and the
//! compositor keeps straight alpha. On the GPU paths ([`metal`]) the
//! surface is the render target, the blend writes premultiplied pixels
//! directly, and the draw is committed with a completion handler: when
//! the GPU is done, the handler hands the surface to the main thread
//! through the view's [`Waker`] and wakes the responder. A draw asked for
//! while one is in flight answers [`Drawn::Busy`], and the responder tries
//! again on the wake.
//!
//! The crate compiles to nothing off macOS, the `baud-isa-aarch64` pattern
//! for a target-bound crate. `unsafe` is denied here and allowed in twelve
//! modules: [`metal`], where every Metal call that takes a pointer needs it;
//! [`print`], for the three print-panel calls the bindings mark so;
//! [`lights`], for the one bitmap the activity lights are shown in;
//! [`notify`], for the messages it sends by name; [`settings`],
//! [`launch`], [`closing`] and [`full_screen`], each for a notification's
//! observer; [`keyup`], for the key-up monitor's registration and
//! handler; [`hover`], for its tracking areas' owner; [`about`], for the
//! panel's film; and [`announce`], for VoiceOver's constants and the post.
#![cfg(target_os = "macos")]
#![deny(unsafe_code)]

pub mod about;
pub mod announce;
pub mod bell;
pub mod clipboard;
pub mod closing;
pub mod display_link;
pub mod full_screen;
pub mod hover;
pub mod journal;
pub mod keyup;
pub mod launch;
pub mod lights;
pub mod link;
pub mod metal;
pub mod notify;
pub mod print;
#[cfg(feature = "effect-probe")]
#[allow(unsafe_code)]
pub mod probe;
pub mod redraw;
#[cfg(feature = "effect-probe")]
pub mod rig_call;
/// The copy of the screen each frame's set holds, kept in `baud_render`
/// where the cell frame is built (`baud_render::screens`).
pub mod screens {
    pub use baud_render::screens::*;
}
pub mod selection;
pub mod settings;
pub mod turn;
pub mod unwind;
pub mod window;

pub use journal::Landing;
#[cfg(feature = "gpu-fault")]
pub use metal::Inject;
pub use metal::{
    Build, Came, Cause, CellInput, CommitId, Completion, Deadline, Done, Drawn, EffectBuild,
    EffectPipes, ErrorNumber, Fault, FaultCode, Faults, Flying, GaveUp, Gpu, GpuFrame, GpuUse,
    GridBuffer, InFlight, Landed, Late, MapBuffer, Memory, Overdue, OverlayBuffer, StatusWord,
    StillBusy, StylesBuffer, Target, Waited,
};

use baud_domain::{FrameGen, HostTime, SwapEpoch};
use baud_font::Px;
use baud_render::{Argb, Renderer};
use journal::{Counts, FrameTag, Journal, Reshape, SetIx};
pub use macos_liquid_glass::drawable::ColorSpace as SurfaceSpace;
/// What a [`Wake`] is handed to show, so a caller outside the app -- a
/// bench driving the real frame loop -- can implement one.
pub use macos_liquid_glass::drawable::SurfaceHandle;
use macos_liquid_glass::drawable::{ColorSpace, DrawableView, Surface, Waker};
use std::sync::Arc;

/// Who hears that a frame landed: the view's [`Waker`] in the app, and
/// whatever a test watches with. Both calls come from the GPU's completion
/// thread.
pub trait Wake: Send + Sync + 'static {
    /// The surface to put on the layer now.
    fn present(&self, surface: &SurfaceHandle);
    /// Wake the responder.
    fn wake(&self);
}

impl Wake for Waker {
    fn present(&self, surface: &SurfaceHandle) {
        Waker::present(self, surface);
    }

    fn wake(&self) {
        Waker::wake(self);
    }
}

use redraw::{SURFACES, SurfaceIx};

/// Which of the surfaces is drawn next.
#[derive(Clone, Copy)]
struct Next(usize);

/// **How the first frame at launch is waited for**: for at most
/// [`Deadline::LAUNCH`], and an answer after that wakes `view`'s responder,
/// whose window is open by then on a frame drawn on the CPU and draws
/// again on the GPU once its frame is no longer in flight.
fn launch(view: &DrawableView) -> Done {
    let waker = view.waker();
    Done::WithinThen(Deadline::LAUNCH, Arc::new(move || waker.wake()))
}

/// Who draws the frame into a surface.
enum Draw {
    /// The compositor paints, and `present` copies the frame in.
    Cpu,
    /// Metal renders into the surface through a target per surface. Boxed,
    /// because a `Gpu` is a dozen words of device state beside a unit
    /// variant.
    Gpu { gpu: Box<Gpu>, targets: Vec<Target> },
}

/// Which of the two paths a presenter takes, for the caller that has to
/// call `present` or `draw` accordingly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Cpu,
    Gpu,
}

pub struct Presenter {
    surfaces: Vec<Surface>,
    next: Next,
    width: Px,
    height: Px,
    draw: Draw,
    /// What the surfaces' bytes mean; a resize keeps it.
    space: ColorSpace,
    /// Which surface set these are. Shared with every frame in flight and
    /// carried across a resize, because a frame committed against the old
    /// set lands after the new one exists and has to be able to say so.
    epoch: Arc<SwapEpoch>,
    /// What the present path did, carried across a resize for the same
    /// reason: a completion counts itself, and the presenter it was
    /// committed by may be gone by then.
    journal: Arc<Journal>,
    /// Which frame the next commit is.
    frame: FrameGen,
    /// Which frame each surface holds and what the frames since changed,
    /// so a GPU frame draws only what its surface lacks. New with the
    /// surfaces, which hold nothing.
    ages: redraw::Ages,
    /// The CPU path's: the pixel rows of each surface that are not the last
    /// frame's, so a frame copies only those -- the Wayland presenter's
    /// rule (`baud-present-wayland`'s `damage.rs`) -- and the margin colour
    /// and frame size the surfaces were filled for.
    stale: Vec<Stale>,
    filled: Option<(Argb, usize, usize)>,
}

/// The pixel rows of a surface that are not the last frame's: all of them,
/// or these, one bit a row.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Stale {
    Whole,
    Rows(Vec<u64>),
}

/// What a CPU frame changed of the one before it, in the frame's pixel
/// rows: everything, or these bands and no pixel outside them
/// ([`baud_render::Renderer::painted`]).
#[derive(Clone, Copy, Debug)]
pub enum Changed<'a> {
    Whole,
    Bands(&'a [baud_render::Band]),
}

/// Whether a frame reached the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Presented {
    Yes,
    /// The surface's lock was refused, the sizes disagree, or the presenter
    /// is on the other path.
    No,
}

impl Presenter {
    /// Surfaces of `width` by `height` pixels for the CPU path; `None`
    /// when the kernel declines.
    pub fn new(width: Px, height: Px, space: ColorSpace) -> Option<Presenter> {
        Presenter::build(width, height, space, Draw::Cpu, Shared::fresh())
    }

    /// Surfaces of `width` by `height` pixels with a Metal target over
    /// each; `None` when the kernel declines or there is no Metal device.
    pub fn with_gpu(width: Px, height: Px, space: ColorSpace) -> Option<Presenter> {
        Presenter::over(Gpu::new()?, width, height, space)
    }

    /// The same over a `Gpu` the caller already made -- because it lent
    /// the terminal its grid memory before there was a window to size.
    pub fn over(gpu: Gpu, width: Px, height: Px, space: ColorSpace) -> Option<Presenter> {
        Presenter::build(
            width,
            height,
            space,
            Draw::Gpu {
                gpu: Box::new(gpu),
                targets: Vec::new(),
            },
            Shared::fresh(),
        )
    }

    fn build(
        width: Px,
        height: Px,
        space: ColorSpace,
        draw: Draw,
        shared: Shared,
    ) -> Option<Presenter> {
        let (w, h) = (slots(width), slots(height));
        let surfaces = (0..SURFACES)
            .map(|_| Surface::in_space(w, h, space))
            .collect::<Option<Vec<_>>>()?;
        let draw = match draw {
            Draw::Cpu => Draw::Cpu,
            Draw::Gpu { gpu, .. } => {
                let targets = surfaces
                    .iter()
                    .map(|s| gpu.target(s.io_surface(), width, height))
                    .collect::<Option<Vec<_>>>()?;
                Draw::Gpu { gpu, targets }
            }
        };
        // **The surfaces exist, so the set they belong to is new.** Every
        // frame committed against the previous one is superseded from here,
        // whatever it was drawn at -- which is the case a size comparison
        // cannot see, since a window dragged out and back leaves two
        // different sets that are the same shape.
        shared.epoch.bump();
        Some(Presenter {
            surfaces,
            next: Next(0),
            width,
            height,
            draw,
            space,
            epoch: shared.epoch,
            journal: shared.journal,
            frame: shared.frame,
            ages: redraw::Ages::new(),
            stale: vec![Stale::Whole; SURFACES],
            filled: None,
        })
    }

    pub fn size(&self) -> (Px, Px) {
        (self.width, self.height)
    }

    pub fn mode(&self) -> Mode {
        match self.draw {
            Draw::Cpu => Mode::Cpu,
            Draw::Gpu { .. } => Mode::Gpu,
        }
    }

    /// The device's memory arrangement, on the GPU path.
    pub fn memory(&self) -> Option<Memory> {
        match &self.draw {
            Draw::Cpu => None,
            Draw::Gpu { gpu, .. } => Some(gpu.unified_memory()),
        }
    }

    /// Whether the GPU is still reading a frame's buffers. The CPU path is
    /// never in flight.
    pub fn in_flight(&self) -> InFlight {
        match &self.draw {
            Draw::Cpu => InFlight::Idle,
            Draw::Gpu { gpu, .. } => gpu.in_flight(),
        }
    }

    /// Replaces the surfaces at a new size, keeping the path; the old ones
    /// are released once the layer stops showing them.
    pub fn resize(&mut self, width: Px, height: Px) -> Option<()> {
        let draw = core::mem::replace(&mut self.draw, Draw::Cpu);
        let shared = Shared {
            epoch: Arc::clone(&self.epoch),
            journal: Arc::clone(&self.journal),
            frame: self.frame,
        };
        *self = Presenter::build(width, height, self.space, draw, shared)?;
        Some(())
    }

    /// What the GPU's completions have counted, on the GPU path: the frames
    /// that failed, by code, the warm-up pass, and the slowest frame.
    pub fn faults(&self) -> Option<Faults> {
        self.gpu().map(Gpu::faults)
    }

    /// **Waits until no frame is in flight, for at most `bound`**
    /// ([`Gpu::idle_within`]), for a bench; at once on the CPU path.
    pub fn idle_within(&self, bound: Deadline) -> Result<(), StillBusy> {
        match self.gpu() {
            Some(g) => g.idle_within(bound),
            None => Ok(()),
        }
    }

    /// What the present path has done: frames landed, shown, and superseded.
    /// Survives a resize, because the frames a resize supersedes are exactly
    /// the ones worth counting.
    pub fn counts(&self) -> Counts {
        self.journal.counts()
    }

    /// The GPU end of the frame whose surface the view is presenting now,
    /// taken once per present; see [`journal::Journal::presented_end`].
    pub fn presented_end(&self) -> Option<HostTime> {
        self.journal.presented_end()
    }

    /// The tag the next commit carries, and the advance of the frame
    /// number. Taking a tag *is* committing, so this is where a set stops
    /// being free: the frame that holds it is the one this mints.
    fn tag(&mut self, set: Option<SetIx>) -> FrameTag {
        self.frame = self.frame.next();
        self.journal.commit(set, self.frame);
        FrameTag {
            frame: self.frame,
            swap: self.epoch.current(),
            set,
        }
    }

    /// The set to build the next cell-path frame into, or none while every
    /// set is held by a frame that has not landed -- in which case there
    /// is nowhere to build that a live frame is not reading.
    pub fn building(&self) -> Option<SetIx> {
        self.journal.wheel().building()
    }

    /// Whether the memory that has no ring -- the terminal's grid -- may be
    /// written now, or a frame in flight is reading it and the reshape has
    /// to wait for the completion. Asking counts a `Wait` as a deferral, so
    /// the frame report carries it.
    pub fn reshape(&self) -> Reshape {
        self.journal.reshape()
    }

    /// Copies `frame` -- `stride` words a row, `0xAARRGGBB` straight alpha,
    /// at least the presenter's size -- into the next surface premultiplied,
    /// and shows it on `view`. The CPU path only.
    pub fn present(
        &mut self,
        view: &DrawableView,
        frame: &[u32],
        stride: Px,
        clear: Argb,
    ) -> Presented {
        self.present_changed(view, frame, stride, clear, Changed::Whole)
    }

    /// [`Presenter::present`] of a frame that changed only `changed` of
    /// the one before: the next surface is brought up to date in the rows
    /// any frame since it was last copied changed, and no other.
    pub fn present_changed(
        &mut self,
        view: &DrawableView,
        frame: &[u32],
        stride: Px,
        clear: Argb,
        changed: Changed<'_>,
    ) -> Presented {
        let at = self.next.0;
        if self.copy_changed(frame, stride, clear, changed) == Presented::No {
            return Presented::No;
        }
        view.set_surface(self.surfaces[at].io_surface());
        Presented::Yes
    }

    /// The copy [`Presenter::present_changed`] shows, into the next surface,
    /// and the surface after made the next. Public for the oracle that
    /// holds it to a whole copy, which needs no view.
    pub fn copy_changed(
        &mut self,
        frame: &[u32],
        stride: Px,
        clear: Argb,
        changed: Changed<'_>,
    ) -> Presented {
        if self.mode() != Mode::Cpu {
            return Presented::No;
        }
        self.copy(frame, stride, clear, changed)
    }

    /// **A frame drawn on the CPU, shown on the GPU path**: for the first
    /// frame at launch when the GPU did not draw it by its bound
    /// ([`Presenter::draw_now`]), so the window never opens with a surface
    /// nothing drew. Copied whole into the next surface -- never the one
    /// the GPU's frame went to, which [`Presenter::draw_now`] has already
    /// passed -- which is then shown on `view`, and is missing everything
    /// for the GPU's next frame into it, since the GPU did not draw it.
    /// [`Presenter::present`] on the CPU path.
    pub fn present_on_cpu(
        &mut self,
        view: &DrawableView,
        frame: &[u32],
        stride: Px,
        clear: Argb,
    ) -> Presented {
        if self.mode() == Mode::Cpu {
            return self.present(view, frame, stride, clear);
        }
        let at = self.next.0;
        if self.copy(frame, stride, clear, Changed::Whole) == Presented::No {
            return Presented::No;
        }
        self.ages.forget(SurfaceIx(at));
        view.set_surface(self.surfaces[at].io_surface());
        Presented::Yes
    }

    /// The copy into the next surface, on either path.
    fn copy(&mut self, frame: &[u32], stride: Px, clear: Argb, changed: Changed<'_>) -> Presented {
        // The surface is the whole view; the grid frame is centred in it and
        // the margin is filled with the terminal background, so the gutter
        // is the same colour as a default cell rather than a see-through
        // edge -- the letterbox the GPU paths do with a clear and a viewport.
        let (sw, sh) = (slots(self.width), slots(self.height));
        let fw = slots(stride);
        if fw == 0 || fw > sw {
            return Presented::No;
        }
        let fh = frame.len() / fw;
        if fh > sh {
            return Presented::No;
        }
        let pad_x = (sw - fw) / 2;
        let pad_y = (sh - fh) / 2;
        let bg = premultiply(clear).0;
        // A new margin colour or frame size refills every surface whole.
        let changed = match self.filled == Some((clear, fw, fh)) {
            true => changed,
            false => Changed::Whole,
        };
        let words = fh.div_ceil(64);
        for stale in &mut self.stale {
            match (changed, &mut *stale) {
                (_, Stale::Whole) => {}
                (Changed::Whole, s) => *s = Stale::Whole,
                (Changed::Bands(bands), Stale::Rows(w)) => {
                    w.resize(words, 0);
                    for b in bands {
                        for y in slots(b.top).min(fh)..slots(b.bottom).min(fh) {
                            w[y / 64] |= 1 << (y % 64);
                        }
                    }
                }
            }
        }
        let at = self.next.0;
        let surface = &self.surfaces[at];
        let stale = &self.stale[at];
        let copy_row = |pixels: &mut [u32], dst_stride: usize, y: usize| {
            let src = &frame[y * fw..y * fw + fw];
            let base = (y + pad_y) * dst_stride + pad_x;
            let dst = &mut pixels[base..base + fw];
            for (d, &s) in dst.iter_mut().zip(src) {
                *d = premultiply(Argb(s)).0;
            }
        };
        let ok = surface.write(|pixels, dst_stride| match stale {
            Stale::Whole => {
                for p in pixels.iter_mut() {
                    *p = bg;
                }
                for y in 0..fh {
                    copy_row(pixels, dst_stride, y);
                }
            }
            Stale::Rows(w) => {
                for y in
                    (0..fh).filter(|&y| w.get(y / 64).is_some_and(|b| b & (1 << (y % 64)) != 0))
                {
                    copy_row(pixels, dst_stride, y);
                }
            }
        });
        if !ok {
            return Presented::No;
        }
        self.stale[at] = Stale::Rows(vec![0; words]);
        self.filled = Some((clear, fw, fh));
        self.next = Next((at + 1) % SURFACES);
        Presented::Yes
    }

    /// Surface `i`'s pixels, row-major at its stride: what the CPU path's
    /// oracle compares.
    pub fn surface_pixels(&self, at: SurfaceIx) -> Option<Vec<u32>> {
        let mut out = Vec::new();
        self.surfaces
            .get(at.0)?
            .write(|p, _| out.extend_from_slice(p))
            .then_some(out)
    }

    /// What the GPU does when a frame into the next surface is done: hand
    /// that surface to the main thread and wake the responder, through the
    /// view's waker, from the GPU's thread. The tag comes back with it, for
    /// [`Presenter::settle`] to give back if the GPU never takes the frame.
    ///
    /// **A frame that failed is not presented**: the layer keeps the last
    /// surface that drew, the surface the failed frame went to is marked
    /// to be drawn whole when it comes round ([`Journal::fail`]), and the
    /// responder is woken so the next frame is drawn.
    fn on_done<W: Wake>(&mut self, waker: W, set: Option<SetIx>) -> (Done, FrameTag) {
        let surface = SurfaceIx(self.next.0);
        let handle = self.surfaces[surface.0].handle();
        let tag = self.tag(set);
        let epoch = Arc::clone(&self.epoch);
        let journal = Arc::clone(&self.journal);
        let done = Done::Notify(Arc::new(move |completion| {
            match completion {
                Completion::Done(landing) => {
                    // **Recorded, and shown anyway.** The verdict is taken
                    // here and deliberately not acted on: a count of frames
                    // that reached the layer after their surfaces were
                    // replaced is only a measurement of the bug while the
                    // bug is still happening. Acting on it is a separate
                    // change, against these numbers.
                    let fate = journal.land(tag, &epoch, landing);
                    if journal::admits(fate) == journal::Show::Now {
                        waker.present(&handle);
                    }
                }
                Completion::Failed(_) => journal.fail(tag, Some(surface)),
            }
            waker.wake();
        }));
        (done, tag)
    }

    /// The surfaces failed frames went to since the last draw, each left
    /// missing everything.
    fn take_lost(&mut self) {
        let lost = self.journal.take_lost();
        for s in (0..SURFACES).map(SurfaceIx).filter(|s| lost.holds(*s)) {
            self.ages.forget(s);
        }
    }

    /// A frame tagged, drawn with `draw` into the next surface, and settled:
    /// the one way both GPU paths commit, so neither can mint a tag the
    /// other would give back.
    fn commit_with<W: Wake>(
        &mut self,
        waker: W,
        set: Option<SetIx>,
        draw: impl FnOnce(&mut Gpu, &Target, Done, baud_render::Redraw) -> Option<Drawn>,
    ) -> Option<Drawn> {
        let (done, tag) = self.on_done(waker, set);
        self.take_lost();
        let missing = self.ages.missing(SurfaceIx(self.next.0));
        let drawn = match &mut self.draw {
            Draw::Gpu { gpu, targets } => targets
                .get(self.next.0)
                .and_then(|target| draw(gpu, target, done, missing)),
            Draw::Cpu => None,
        };
        self.settle(tag, drawn)
    }

    /// A drawn frame will land through its completion. Anything else --
    /// `Busy`, a resource the draw could not make, no target -- committed
    /// nothing, so nothing will land it, and its tag is given back.
    fn settle(&mut self, tag: FrameTag, drawn: Option<Drawn>) -> Option<Drawn> {
        if !matches!(drawn, Some(Drawn::Frame(_))) {
            self.journal.abandon(tag);
        }
        drawn.map(|d| self.committed(d))
    }

    /// A frame committed into the next surface, which is then the one
    /// after; a frame waited for that failed, or is past its bound, leaves
    /// its surface missing everything.
    fn committed(&mut self, drawn: Drawn) -> Drawn {
        if let Drawn::Frame(f) = &drawn {
            let at = SurfaceIx(self.next.0);
            self.ages.drew(at, f.changed);
            if let Waited::Failed(_) | Waited::Overdue(_) = f.waited {
                self.ages.forget(at);
            }
            self.next = Next((self.next.0 + 1) % SURFACES);
        }
        drawn
    }

    /// Commits the renderer's converted frame -- the instance path -- into
    /// the next surface, to be shown when the GPU is done. The GPU path
    /// only; `None` otherwise, or when a Metal resource could not be made.
    pub fn draw<W: Wake>(&mut self, waker: W, renderer: &mut Renderer<'_, '_>) -> Option<Drawn> {
        // The instance path converts on the main thread and builds into
        // the presenter's own ring, so it holds none of the caller's sets.
        self.commit_with(waker, None, |gpu, target, done, missing| {
            gpu.draw_since(renderer, target, done, missing)
        })
    }

    /// The instance path drawn and shown now: the GPU waited for, for at
    /// most [`Deadline::LAUNCH`], and the surface set on `view` on this
    /// thread if the frame drew. For the first frame at launch, before a run
    /// loop exists to carry the completion; never in the loop. A frame that
    /// did not draw by the bound, failed or unanswered, is not shown: the
    /// caller draws that frame on the CPU instead
    /// ([`Presenter::present_on_cpu`]); one that lands after the bound
    /// wakes `view`'s responder.
    pub fn draw_now(
        &mut self,
        view: &DrawableView,
        renderer: &mut Renderer<'_, '_>,
    ) -> Option<Drawn> {
        self.take_lost();
        let slot = self.next.0;
        let Draw::Gpu { gpu, targets } = &mut self.draw else {
            return None;
        };
        let missing = self.ages.missing(SurfaceIx(slot));
        let drawn = gpu.draw_since(renderer, targets.get(slot)?, launch(view), missing)?;
        // Shown only if it drew: a frame that failed leaves the layer as
        // it was.
        if let Drawn::Frame(f) = &drawn {
            if f.waited.landed().is_ok() {
                view.set_surface(self.surfaces[slot].io_surface());
            }
        }
        Some(self.committed(drawn))
    }

    /// The cell path drawn and shown now; see [`Presenter::draw_now`].
    pub fn draw_cells_now(
        &mut self,
        view: &DrawableView,
        renderer: &mut Renderer<'_, '_>,
        input: &CellInput<'_>,
    ) -> Option<Drawn> {
        self.take_lost();
        let slot = self.next.0;
        let Draw::Gpu { gpu, targets } = &mut self.draw else {
            return None;
        };
        let missing = self.ages.missing(SurfaceIx(slot));
        let drawn =
            gpu.draw_cells_since(renderer, input, targets.get(slot)?, launch(view), missing)?;
        // Shown only if it drew: a frame that failed leaves the layer as
        // it was.
        if let Drawn::Frame(f) = &drawn {
            if f.waited.landed().is_ok() {
                view.set_surface(self.surfaces[slot].io_surface());
            }
        }
        Some(self.committed(drawn))
    }

    /// Commits the raw grid -- the cell path -- into the next surface, to be
    /// shown when the GPU is done. The GPU path only.
    pub fn draw_cells<W: Wake>(
        &mut self,
        waker: W,
        renderer: &mut Renderer<'_, '_>,
        input: &CellInput<'_>,
    ) -> Option<Drawn> {
        self.commit_with(waker, Some(input.set), |gpu, target, done, missing| {
            gpu.draw_cells_since(renderer, input, target, done, missing)
        })
    }

    /// **A screen switch's old screen, kept for its slide**, on the GPU
    /// path: the renderer's frame drawn with its cursor hidden into a
    /// texture of the targets' size ([`Gpu::draw_kept`]), and no surface,
    /// so nothing is shown; the completion wakes the responder through the
    /// view's waker to draw the frame after it. `None` on the CPU path,
    /// which keeps its own frame buffer, or when a resource could not be
    /// made; [`Drawn::Busy`] while a frame is in flight.
    pub fn keep_frame<W: Wake>(
        &mut self,
        waker: W,
        renderer: &mut Renderer<'_, '_>,
    ) -> Option<Drawn> {
        self.keep_with(waker, None, |gpu, size, done| {
            gpu.draw_kept(renderer, size, done)
        })
    }

    /// [`Presenter::keep_frame`] on the cell path: `input` the last frame,
    /// built again with no cursor ([`Gpu::draw_cells_kept`]), its set held
    /// until the GPU has read it.
    pub fn keep_cells<W: Wake>(
        &mut self,
        waker: W,
        renderer: &mut Renderer<'_, '_>,
        input: &CellInput<'_>,
    ) -> Option<Drawn> {
        self.keep_with(waker, Some(input.set), |gpu, size, done| {
            gpu.draw_cells_kept(renderer, input, size, done)
        })
    }

    /// A kept frame tagged and drawn with `draw` at the targets' size: its
    /// set held until it lands, and given back if it never will, as a
    /// surface's frame is, but presented nowhere and counted as no frame
    /// shown ([`Journal::land_kept`]).
    fn keep_with<W: Wake>(
        &mut self,
        waker: W,
        set: Option<SetIx>,
        draw: impl FnOnce(&mut Gpu, (Px, Px), Done) -> Option<Drawn>,
    ) -> Option<Drawn> {
        let tag = self.tag(set);
        let journal = Arc::clone(&self.journal);
        let done = Done::Notify(Arc::new(move |completion| {
            match completion {
                Completion::Done(_) => journal.land_kept(tag),
                // The GPU drops the slide over it at the next draw.
                Completion::Failed(_) => journal.fail(tag, None),
            }
            waker.wake();
        }));
        let drawn = match &mut self.draw {
            Draw::Gpu { gpu, targets } => targets
                .first()
                .map(Target::size)
                .and_then(|size| draw(gpu, size, done)),
            Draw::Cpu => None,
        };
        if !matches!(drawn, Some(Drawn::Frame(_))) {
            self.journal.abandon(tag);
        }
        drawn
    }

    /// Where a screen switch's slide is for the next GPU frame, or none,
    /// which ends it ([`Gpu::set_slide`]).
    pub fn set_slide(&mut self, at: Option<baud_render::ScreenSlide>) {
        if let Draw::Gpu { gpu, .. } = &mut self.draw {
            gpu.set_slide(at);
        }
    }

    /// **What builds `effect`'s pipelines off this thread**, on the GPU
    /// path ([`Gpu::effect_build`]); the CPU path has none to build.
    pub fn effect_build(
        &self,
        effect: baud_render::effect::Effect,
    ) -> Option<crate::metal::EffectBuild> {
        match &self.draw {
            Draw::Cpu => None,
            Draw::Gpu { gpu, .. } => gpu.effect_build(effect),
        }
    }

    /// The pipelines built off this thread, taken ([`Gpu::adopt`]).
    pub fn adopt(&mut self, built: crate::metal::EffectPipes) {
        if let Draw::Gpu { gpu, .. } = &mut self.draw {
            gpu.adopt(built);
        }
    }

    /// Whether `effect`'s pipeline is built ([`Gpu::effect_built`]),
    /// always on the CPU path, which needs none.
    pub fn effect_built(&self, effect: baud_render::effect::Effect) -> bool {
        match &self.draw {
            Draw::Cpu => true,
            Draw::Gpu { gpu, .. } => gpu.effect_built(effect),
        }
    }

    /// The GPU, to lend memory from, on the GPU path.
    pub fn gpu(&self) -> Option<&Gpu> {
        match &self.draw {
            Draw::Cpu => None,
            Draw::Gpu { gpu, .. } => Some(gpu),
        }
    }
}

/// What a presenter hands to its successor across a resize: the frames it
/// has in flight outlive it, and they name the epoch and count themselves in
/// the journal, so neither may be rebuilt with the surfaces.
struct Shared {
    epoch: Arc<SwapEpoch>,
    journal: Arc<Journal>,
    frame: FrameGen,
}

impl Shared {
    fn fresh() -> Shared {
        Shared {
            epoch: Arc::new(SwapEpoch::new()),
            journal: Arc::new(Journal::default()),
            frame: FrameGen::FIRST,
        }
    }
}

/// A straight-alpha pixel as Core Animation wants it: each colour channel
/// scaled by the alpha, rounded -- `baud_domain::premultiply`, the function
/// the Wayland presenter calls too. Public so the pixel oracle presents the
/// CPU arm's frame through this function and not a copy of it.
pub fn premultiply(p: Argb) -> Argb {
    Argb(baud_domain::premultiply(p.0))
}

/// A pixel count as a slice index: the one place the conversion is spelled.
pub(crate) fn slots(p: Px) -> usize {
    usize::try_from(p.value()).unwrap_or(usize::MAX)
}
