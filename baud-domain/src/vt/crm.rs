//! The hold CONTROL REPRESENTATION MODE's GRAPHIC state keeps a control
//! sequence in. `[[source: ecma48]]` §7.2.2: "All control functions, except
//! RESET MODE (RM), are treated as graphic characters", and a sequence is
//! RM only at its final byte, so its bytes are held from its introducer to
//! that byte, then performed if it is RM and shown otherwise.
//!
//! **The hold is a fixed array**, [`CrmHold::CAPACITY`] bytes, and remote
//! input decides what fills it, so it never grows: a byte past the capacity
//! gives back what is held, to be shown, and every later byte of the
//! sequence passes through to be shown as it arrives. Keeping a byte is one
//! store and one compare, with no scan of what is held.
//!
//! **An over-long sequence is shown, never performed.** ECMA-48 bounds
//! neither a sequence's parameters (§5.4.3, "neither the maximum number of
//! values nor the order in which the corresponding actions are performed are
//! defined by this Standard") nor its intermediates (§5.4, "The number of
//! Intermediate Bytes is not limited by this Standard"), so the bound is
//! baud's. `[[source: dec-std-070]]` 070-3 §3.5.3 requires "up to 16
//! parameters", and its reference parser does not perform a sequence that
//! outgrows it: "If the stack overflows, the parser will re-initialize the
//! stack, and return with event PARSE ERROR". So a sequence past the hold is
//! not a control function baud recognises, RM included, and under GRAPHIC
//! what is received is shown. Sixteen parameters of five digits and three
//! intermediates take about a hundred bytes, so only padding or a list past
//! sixteen reaches 256.

/// How many bytes of the sequence the hold keeps now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct HeldLen(u16);

/// Whether the sequence has outgrown the hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reach {
    /// Every byte so far is held.
    Within,
    /// The held bytes were given back to be shown, and the rest pass through.
    Spilled,
}

/// One control sequence's bytes under CRM GRAPHIC, held until its final byte.
///
/// Its fields are private, so only [`CrmHold::new`] and [`CrmHold::hold`]
/// set them and the count never passes the array:
///
/// ```compile_fail,E0451
/// let _ = baud_domain::CrmHold {
///     held: [0; 256],
///     ..baud_domain::CrmHold::new()
/// };
/// ```
///
/// Holding and ending a sequence that fits:
///
/// ```
/// use baud_domain::{CrmEnd, CrmHold, CrmStep};
/// let mut h = CrmHold::new();
/// for &b in b"\x1b[3l" {
///     assert!(matches!(h.hold(b), CrmStep::Kept));
/// }
/// assert!(matches!(h.finish(), CrmEnd::Whole(b"\x1b[3l")));
/// ```
#[derive(Clone, Debug)]
pub struct CrmHold {
    held: [u8; CrmHold::CAPACITY],
    len: HeldLen,
    reach: Reach,
}

/// What [`CrmHold::hold`] did with a byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrmStep<'a> {
    /// Held, with the rest of the sequence.
    Kept,
    /// The hold was full: these are every byte held so far, to be shown now,
    /// and the byte given is shown after them.
    Spill(&'a [u8]),
    /// The sequence spilled before this byte: show it.
    Pass,
}

/// What [`CrmHold::finish`] gives back at the sequence's end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrmEnd<'a> {
    /// The whole sequence, held: performed if it is RM, shown otherwise.
    Whole(&'a [u8]),
    /// It spilled, and every byte was already given back to be shown.
    Shown,
}

const _: () = assert!(CrmHold::CAPACITY <= u16::MAX as usize);

impl CrmHold {
    /// The bytes one held sequence may take, its introducer and final byte
    /// included.
    pub const CAPACITY: usize = 256;

    /// An empty hold.
    pub const fn new() -> CrmHold {
        CrmHold {
            held: [0; CrmHold::CAPACITY],
            len: HeldLen(0),
            reach: Reach::Within,
        }
    }

    /// Holds `b`, the sequence's next byte, or says it is to be shown.
    #[inline]
    pub fn hold(&mut self, b: u8) -> CrmStep<'_> {
        if self.reach == Reach::Spilled {
            return CrmStep::Pass;
        }
        let n = usize::from(self.len.0);
        match self.held.get_mut(n) {
            Some(slot) => {
                *slot = b;
                self.len = HeldLen(self.len.0 + 1);
                CrmStep::Kept
            }
            None => {
                self.reach = Reach::Spilled;
                self.len = HeldLen(0);
                CrmStep::Spill(&self.held[..n])
            }
        }
    }

    /// The bytes held and not yet given back.
    #[inline]
    pub fn held(&self) -> &[u8] {
        &self.held[..usize::from(self.len.0)]
    }

    /// Ends the sequence and empties the hold for the next one.
    #[inline]
    pub fn finish(&mut self) -> CrmEnd<'_> {
        let n = usize::from(self.len.0);
        let reach = self.reach;
        self.len = HeldLen(0);
        self.reach = Reach::Within;
        match reach {
            Reach::Within => CrmEnd::Whole(&self.held[..n]),
            Reach::Spilled => CrmEnd::Shown,
        }
    }
}

impl Default for CrmHold {
    fn default() -> CrmHold {
        CrmHold::new()
    }
}
