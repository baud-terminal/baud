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
//! **Three surfaces in rotation, and the renderer keeps its own frame.**
//! A surface being shown must not be written, so on the CPU path the frame
//! is painted into the renderer's persistent buffer -- which is what
//! row-granular damage needs, a target that still holds the last frame --
//! and copied whole into the next surface, premultiplied on the way because
//! Core Animation reads `IOSurface` pixels premultiplied and the compositor
//! keeps straight alpha. That copy is the price of the split and it is
//! measured by the `present` column of `baud-bench`'s `frame` bench once
//! one exists. On the GPU path ([`metal`]) the surface is the render
//! target and the blend writes premultiplied pixels directly, so there is
//! no copy.
//!
//! The crate compiles to nothing off macOS, the `baud-isa-aarch64` pattern
//! for a target-bound crate. `unsafe` is denied here and allowed in one
//! module, [`metal`], where every Metal call that takes a pointer needs it.
#![cfg(target_os = "macos")]
#![deny(unsafe_code)]

pub mod metal;

pub use metal::{Gpu, GpuFrame, Memory, Target};

use baud_font::Px;
use baud_render::{Argb, Renderer};
use macos_liquid_glass::drawable::{DrawableView, Surface};

/// How many surfaces rotate: one showing, one the compositor may still be
/// reading for the previous transaction, one being written.
const SURFACES: usize = 3;

/// Which of the surfaces is written next.
#[derive(Clone, Copy)]
struct Next(usize);

/// Who draws the frame into a surface.
enum Draw {
    /// The compositor paints, and `present` copies the frame in.
    Cpu,
    /// Metal renders into the surface through a target per surface.
    Gpu { gpu: Gpu, targets: Vec<Target> },
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
        let gpu = Gpu::new()?;
        Presenter::build(
            width,
            height,
            Draw::Gpu {
                gpu,
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

    /// Draws the renderer's current frame into the next surface on the GPU
    /// and shows it on `view`. The GPU path only; `None` otherwise, or when
    /// a Metal resource could not be made.
    pub fn draw(&mut self, view: &DrawableView, renderer: &Renderer<'_, '_>) -> Option<GpuFrame> {
        let Draw::Gpu { gpu, targets } = &mut self.draw else {
            return None;
        };
        let slot = self.next.0;
        let frame = gpu.draw(renderer, targets.get(slot)?)?;
        view.set_surface(self.surfaces[slot].io_surface());
        self.next = Next((slot + 1) % SURFACES);
        Some(frame)
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
