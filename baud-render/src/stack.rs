//! The faces a frame draws with, in fallback order.

use baud_domain::Codepoint;
use baud_font::Face;

/// A face's position in the stack. A byte, because a fallback chain of
/// more than 255 faces is a configuration error rather than a font stack.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FaceIx(u8);

impl FaceIx {
    pub const PRIMARY: FaceIx = FaceIx(0);

    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// An ordered list of faces: the primary first, then the fallbacks tried
/// per cluster in order. Discovery is the caller's; this crate only draws.
pub struct Stack<'f, 'a> {
    faces: &'f [Face<'a>],
}

impl<'f, 'a> Stack<'f, 'a> {
    /// `None` for an empty list or one longer than a byte can index.
    pub fn new(faces: &'f [Face<'a>]) -> Option<Stack<'f, 'a>> {
        (!faces.is_empty() && faces.len() <= usize::from(u8::MAX)).then_some(Stack { faces })
    }

    pub fn primary(&self) -> &'f Face<'a> {
        &self.faces[0]
    }

    pub fn faces(&self) -> &'f [Face<'a>] {
        self.faces
    }

    /// Every face's index, in stack order.
    pub fn each(&self) -> impl Iterator<Item = FaceIx> + '_ {
        (0..self.faces.len())
            .filter_map(|i| u8::try_from(i).ok())
            .map(FaceIx)
    }

    pub fn face(&self, ix: FaceIx) -> &'f Face<'a> {
        self.faces.get(ix.index()).unwrap_or(&self.faces[0])
    }

    /// The first face that maps `c`; the primary when none does, so its
    /// `.notdef` is what a character nobody carries draws as.
    pub fn select(&self, c: Codepoint) -> FaceIx {
        self.faces
            .iter()
            .position(|f| f.glyph(c).is_some())
            .and_then(|i| u8::try_from(i).ok())
            .map_or(FaceIx::PRIMARY, FaceIx)
    }
}
