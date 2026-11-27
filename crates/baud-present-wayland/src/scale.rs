//! **The scale a window's frame is drawn at, and the buffer it makes.**
//!
//! fractional-scale-v1 states it as "the numerator of a fraction with a
//! denominator of 120" (`[[source: wp-fractional-scale-v1]]`), so the
//! scale here is kept in 120ths and every size is integer arithmetic: an
//! integer scale `n` is `n * 120`, and 1.5 is 180. The buffer is the
//! surface "multiplied by the intended scale", and "For toplevel surfaces,
//! the size is rounded halfway away from zero": a 641-pixel surface at 1.5
//! is a 962-pixel buffer, not 961.
//!
//! **Which scale is in force** is the order the protocols give it, as foot
//! orders it (`[[source: foot-2705e36-terminal]]`, `term_update_scale`):
//! the fractional `preferred_scale` once the compositor
//! has sent one and the surface has a viewport to map the buffer through,
//! "A client can submit scaled content by utilizing wp_viewport"; else
//! `wl_surface.preferred_buffer_scale` from version 6, "Before receiving
//! this event the preferred buffer scale for this surface is 1"; else
//! below it the largest `wl_output.scale` of the outputs the surface is on,
//! "It is intended that you pick the same buffer scale as the scale of
//! the output that the surface is displayed on" (`set_buffer_scale`,
//! `[[source: wayland]]`); and one where the surface takes no buffer scale.

use crate::cursor::Scale;
use crate::shm::Px;

/// **A scale in 120ths of a pixel to the surface's point.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferScale(pub u32);

impl BufferScale {
    /// One buffer pixel a surface point.
    pub const ONE: BufferScale = BufferScale(120);

    /// An integer scale, `wl_surface.set_buffer_scale`'s.
    pub fn whole(n: Scale) -> BufferScale {
        BufferScale(n.0.max(1).saturating_mul(120))
    }

    /// Buffer pixels to the surface point.
    pub fn factor(self) -> f64 {
        f64::from(self.0) / 120.0
    }

    /// **A surface length in buffer pixels**: multiplied and "rounded
    /// halfway away from zero", which for a length is half up.
    pub fn buffer(self, surface: Px) -> Px {
        let px = (u64::from(surface.0) * u64::from(self.0) + 60) / 120;
        Px(u32::try_from(px).unwrap_or(u32::MAX))
    }
}

/// **How the scale is told to the compositor**, which a commit carries
/// with the buffer it applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mapped {
    /// fractional-scale-v1's way: "The wl_surface buffer scale should
    /// remain set to 1", and the viewport's destination is the surface
    /// size, the buffer that size times the scale.
    Viewport,
    /// `wl_surface.set_buffer_scale`: "you have to attach a buffer that is
    /// larger (by a factor of scale in each dimension) than the desired
    /// surface size".
    BufferScale(Scale),
}

/// What a commit carries of the scale: the buffer scale, and the
/// viewport's destination where one is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Told {
    pub(crate) buffer_scale: Scale,
    pub(crate) destination: Option<(Px, Px)>,
}

impl Told {
    /// A new surface's: "A newly created surface has its buffer scale set
    /// to 1", and a viewport's destination is "Initially ... unset".
    pub(crate) const FRESH: Told = Told {
        buffer_scale: Scale(1),
        destination: None,
    };

    /// What a commit of a surface `size` big, mapped as `m`, carries.
    pub(crate) fn of(m: Mapped, size: (Px, Px)) -> Told {
        match m {
            Mapped::Viewport => Told {
                buffer_scale: Scale(1),
                destination: Some(size),
            },
            Mapped::BufferScale(n) => Told {
                buffer_scale: n,
                destination: None,
            },
        }
    }
}

/// Whether the surface has a viewport to map a fraction through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Viewport {
    Made,
    None,
}

/// **The scale in force, and how it is mapped**: `fraction` the last
/// `preferred_scale`, taken only with a viewport; else `whole`, the
/// integer scale the surface's version learns.
pub(crate) fn chosen(
    fraction: Option<BufferScale>,
    viewport: Viewport,
    whole: Scale,
) -> (BufferScale, Mapped) {
    match (fraction, viewport) {
        (Some(f), Viewport::Made) if f.0 > 0 => (f, Mapped::Viewport),
        _ => {
            let whole = Scale(whole.0.max(1));
            (BufferScale::whole(whole), Mapped::BufferScale(whole))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_is_the_surface_times_the_scale_rounded_half_away() {
        let at = |s: u32, l: u32| BufferScale(s).buffer(Px(l)).0;
        assert_eq!(at(120, 640), 640);
        assert_eq!(at(240, 640), 1280);
        assert_eq!(at(180, 640), 960);
        // 641 * 1.5 = 961.5: away from zero.
        assert_eq!(at(180, 641), 962);
        // 409 * 1.25 = 511.25 down, 410 * 1.25 = 512.5 up.
        assert_eq!(at(150, 409), 511);
        assert_eq!(at(150, 410), 513);
        assert_eq!(at(180, 0), 0);
    }

    #[test]
    fn the_fraction_needs_a_viewport_and_wins_over_the_whole_scale() {
        let f = Some(BufferScale(180));
        assert_eq!(
            chosen(f, Viewport::Made, Scale(2)),
            (BufferScale(180), Mapped::Viewport)
        );
        assert_eq!(
            chosen(f, Viewport::None, Scale(2)),
            (BufferScale(240), Mapped::BufferScale(Scale(2))),
            "without a viewport the fraction cannot be mapped"
        );
        assert_eq!(
            chosen(None, Viewport::Made, Scale(2)),
            (BufferScale(240), Mapped::BufferScale(Scale(2))),
            "before the compositor says a fraction"
        );
        assert_eq!(
            chosen(None, Viewport::None, Scale(1)),
            (BufferScale::ONE, Mapped::BufferScale(Scale(1)))
        );
        assert_eq!(BufferScale::whole(Scale(3)), BufferScale(360));
        assert_eq!(BufferScale(180).factor(), 1.5);
    }
}
