use crate::*;
use core::marker::PhantomData;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// A font configuration generation. Atlas slots are only meaningful within one.
pub struct FontEpoch<'font> {
    _b: Brand<'font>,
}

/// Index into the glyph atlas — **font-dependent**, and that is legal only
/// because the viewport is disposable. On a font change the viewport
/// re-materialises from spans in 44 us and every slot is regenerated.
///
/// The brand makes that mandatory rather than optional: a `Viewport<'font>`
/// cannot survive into a new epoch.
#[derive(Clone, Copy)]
pub struct AtlasSlot<'font> {
    v: u16,
    _b: Brand<'font>,
}

/// A glyph within one font face, as the shaper resolved it. Font-specific:
/// glyph 47 is a different character in every font, which is why this may
/// never be stored anywhere but the disposable viewport.
#[derive(Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct GlyphId(pub(crate) u16);

impl GlyphId {
    pub fn from_shaper(n: u16) -> GlyphId {
        GlyphId(n)
    }
}

impl<'font> FontEpoch<'font> {
    pub fn slot(&self, g: GlyphId) -> AtlasSlot<'font> {
        AtlasSlot {
            v: g.0,
            _b: self._b,
        }
    }
    /// Named so the widening is visible where the atlas is indexed.
    pub fn index(&self, s: AtlasSlot<'font>) -> usize {
        s.v as usize
    }
}

/// Enter a font scope. A font change means calling this again; anything
/// holding the old epoch's slots will not type-check.
pub fn with_font<R>(f: impl for<'id> FnOnce(FontEpoch<'id>) -> R) -> R {
    f(FontEpoch {
        _b: Brand(PhantomData),
    })
}
