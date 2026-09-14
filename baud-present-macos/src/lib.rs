//! macOS presentation: the window is the owner's `macos-liquid-glass`; this
//! crate supplies what goes inside `set_content_view`.
//!
//! The design is `docs/research/notes/j-rendering.md` D13, with the owner's
//! two macOS calls made: one IOSurface-backed texture that the CPU writes
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
//! for a target-bound crate. `unsafe` is denied here and allowed in one
//! module, [`metal`], where every Metal call that takes a pointer needs it.
#![cfg(target_os = "macos")]
#![deny(unsafe_code)]

pub mod clipboard;
pub mod metal;

pub use metal::{
    CellInput, Done, Drawn, Gpu, GpuFrame, GridBuffer, InFlight, MapBuffer, Memory, OverlayBuffer,
    StylesBuffer, Target,
};

use baud_font::Px;
use baud_render::{Argb, Renderer};
use macos_liquid_glass::drawable::{DrawableView, Surface, Waker};
use std::sync::Arc;

/// How many surfaces rotate: one showing, one the compositor may still be
/// reading for the previous transaction, one being drawn.
const SURFACES: usize = 3;

/// Which of the surfaces is drawn next.
#[derive(Clone, Copy)]
struct Next(usize);

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
    pub fn new(width: Px, height: Px) -> Option<Presenter> {
        Presenter::build(width, height, Draw::Cpu)
    }

    /// Surfaces of `width` by `height` pixels with a Metal target over
    /// each; `None` when the kernel declines or there is no Metal device.
    pub fn with_gpu(width: Px, height: Px) -> Option<Presenter> {
        Presenter::over(Gpu::new()?, width, height)
    }

    /// The same over a `Gpu` the caller already made -- because it lent
    /// the terminal its grid memory before there was a window to size.
    pub fn over(gpu: Gpu, width: Px, height: Px) -> Option<Presenter> {
        Presenter::build(
            width,
            height,
            Draw::Gpu {
                gpu: Box::new(gpu),
                targets: Vec::new(),
            },
        )
    }

    fn build(width: Px, height: Px, draw: Draw) -> Option<Presenter> {
        let (w, h) = (slots(width), slots(height));
        let surfaces = (0..SURFACES)
            .map(|_| Surface::new(w, h))
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
        Some(Presenter {
            surfaces,
            next: Next(0),
            width,
            height,
            draw,
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
        *self = Presenter::build(width, height, draw)?;
        Some(())
    }

    /// Copies `frame` -- `stride` words a row, `0xAARRGGBB` straight alpha,
    /// at least the presenter's size -- into the next surface premultiplied,
    /// and shows it on `view`. The CPU path only.
    pub fn present(&mut self, view: &DrawableView, frame: &[u32], stride: Px) -> Presented {
        if self.mode() != Mode::Cpu {
            return Presented::No;
        }
        let (w, h) = (slots(self.width), slots(self.height));
        let stride = slots(stride);
        if stride < w || frame.len() < stride * h {
            return Presented::No;
        }
        let surface = &self.surfaces[self.next.0];
        let ok = surface.write(|pixels, dst_stride| {
            for y in 0..h {
                let src = &frame[y * stride..y * stride + w];
                let dst = &mut pixels[y * dst_stride..y * dst_stride + w];
                for (d, &s) in dst.iter_mut().zip(src) {
                    *d = premultiply(Argb(s)).0;
                }
            }
        });
        if !ok {
            return Presented::No;
        }
        view.set_surface(surface.io_surface());
        self.next = Next((self.next.0 + 1) % SURFACES);
        Presented::Yes
    }

    /// What the GPU does when a frame into the next surface is done: hand
    /// that surface to the main thread and wake the responder, through the
    /// view's waker, from the GPU's thread.
    fn on_done(&self, waker: Waker) -> Done {
        let handle = self.surfaces[self.next.0].handle();
        Done::Notify(Arc::new(move || {
            waker.present(&handle);
            waker.wake();
        }))
    }

    fn committed(&mut self, drawn: Drawn) -> Drawn {
        if let Drawn::Frame(_) = drawn {
            self.next = Next((self.next.0 + 1) % SURFACES);
        }
        drawn
    }

    /// Commits the renderer's converted frame -- the instance path -- into
    /// the next surface, to be shown when the GPU is done. The GPU path
    /// only; `None` otherwise, or when a Metal resource could not be made.
    pub fn draw(&mut self, waker: Waker, renderer: &mut Renderer<'_, '_>) -> Option<Drawn> {
        let done = self.on_done(waker);
        let Draw::Gpu { gpu, targets } = &mut self.draw else {
            return None;
        };
        let drawn = gpu.draw(renderer, targets.get(self.next.0)?, done)?;
        Some(self.committed(drawn))
    }

    /// The instance path drawn and shown now: the GPU waited for and the
    /// surface set on `view` on this thread. For the first frame at launch,
    /// before a run loop exists to carry the completion; never in the loop.
    pub fn draw_now(
        &mut self,
        view: &DrawableView,
        renderer: &mut Renderer<'_, '_>,
    ) -> Option<Drawn> {
        let slot = self.next.0;
        let Draw::Gpu { gpu, targets } = &mut self.draw else {
            return None;
        };
        let drawn = gpu.draw(renderer, targets.get(slot)?, Done::Wait)?;
        if let Drawn::Frame(_) = drawn {
            view.set_surface(self.surfaces[slot].io_surface());
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
        let slot = self.next.0;
        let Draw::Gpu { gpu, targets } = &mut self.draw else {
            return None;
        };
        let drawn = gpu.draw_cells(renderer, input, targets.get(slot)?, Done::Wait)?;
        if let Drawn::Frame(_) = drawn {
            view.set_surface(self.surfaces[slot].io_surface());
        }
        Some(self.committed(drawn))
    }

    /// Commits the raw grid -- the cell path -- into the next surface, to be
    /// shown when the GPU is done. The GPU path only.
    pub fn draw_cells(
        &mut self,
        waker: Waker,
        renderer: &mut Renderer<'_, '_>,
        input: &CellInput<'_>,
    ) -> Option<Drawn> {
        let done = self.on_done(waker);
        let Draw::Gpu { gpu, targets } = &mut self.draw else {
            return None;
        };
        let drawn = gpu.draw_cells(renderer, input, targets.get(self.next.0)?, done)?;
        Some(self.committed(drawn))
    }

    /// The GPU, to lend memory from, on the GPU path.
    pub fn gpu(&self) -> Option<&Gpu> {
        match &self.draw {
            Draw::Cpu => None,
            Draw::Gpu { gpu, .. } => Some(gpu),
        }
    }
}

/// A straight-alpha pixel as Core Animation wants it: each colour channel
/// scaled by the alpha.
fn premultiply(p: Argb) -> Argb {
    let [a, r, g, b] = p.0.to_be_bytes();
    if a == 0xFF {
        return p;
    }
    let scale =
        |c: u8| -> u8 { u8::try_from((u32::from(c) * u32::from(a) + 127) / 255).unwrap_or(0) };
    Argb(u32::from_be_bytes([a, scale(r), scale(g), scale(b)]))
}

/// A pixel count as a slice index: the one place the conversion is spelled.
pub(crate) fn slots(p: Px) -> usize {
    usize::try_from(p.value()).unwrap_or(usize::MAX)
}
