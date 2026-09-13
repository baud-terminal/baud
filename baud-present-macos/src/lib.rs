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
//! A surface being shown must not be written, so the frame is painted into
//! the renderer's persistent buffer -- which is what row-granular damage
//! needs, a target that still holds the last frame -- and copied whole into
//! the next surface, premultiplied on the way because Core Animation reads
//! `IOSurface` pixels premultiplied and the compositor keeps straight
//! alpha. That copy is the price of the split and it is measured by the
//! `present` column of `baud-bench`'s `frame` bench once one exists.
//!
//! The crate compiles to nothing off macOS, the `baud-isa-aarch64` pattern
//! for a target-bound crate.
#![cfg(target_os = "macos")]
#![forbid(unsafe_code)]

use baud_font::Px;
use baud_render::Argb;
use macos_liquid_glass::drawable::{DrawableView, Surface};

/// How many surfaces rotate: one showing, one the compositor may still be
/// reading for the previous transaction, one being written.
const SURFACES: usize = 3;

/// The surfaces a view shows, in rotation.
/// Which of the surfaces is written next.
#[derive(Clone, Copy)]
struct Next(usize);

pub struct Presenter {
    surfaces: Vec<Surface>,
    next: Next,
    width: Px,
    height: Px,
}

/// Whether a frame reached the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Presented {
    Yes,
    /// The surface's lock was refused, or the sizes disagree.
    No,
}

impl Presenter {
    /// Surfaces of `width` by `height` pixels; `None` when the kernel
    /// declines.
    pub fn new(width: Px, height: Px) -> Option<Presenter> {
        let (w, h) = (slots(width), slots(height));
        let surfaces = (0..SURFACES)
            .map(|_| Surface::new(w, h))
            .collect::<Option<Vec<_>>>()?;
        Some(Presenter {
            surfaces,
            next: Next(0),
            width,
            height,
        })
    }

    pub fn size(&self) -> (Px, Px) {
        (self.width, self.height)
    }

    /// Replaces the surfaces at a new size; the old ones are released once
    /// the layer stops showing them.
    pub fn resize(&mut self, width: Px, height: Px) -> Option<()> {
        *self = Presenter::new(width, height)?;
        Some(())
    }

    /// Copies `frame` -- `stride` words a row, `0xAARRGGBB` straight alpha,
    /// at least the presenter's size -- into the next surface premultiplied,
    /// and shows it on `view`.
    pub fn present(&mut self, view: &DrawableView, frame: &[u32], stride: Px) -> Presented {
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

fn slots(p: Px) -> usize {
    usize::try_from(p.value()).unwrap_or(usize::MAX)
}
