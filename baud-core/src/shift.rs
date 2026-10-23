//! What a GR byte, a single-shifted character and a `DEL` under a
//! 96-character set write.
//!
//! **All out of line**: the per-character text path reads `Paint::charset`,
//! the set GL resolves to, and none of this. GR is read here and nowhere
//! else (`Cold::gr`), a pending single shift is a node of the graph
//! (`Node::SingleShift`), and a `DEL` reaches `gl_del` only when GL holds a
//! 96-character set.

use crate::paint::{Paint, Pen};
use crate::write::glyph_any;
use baud_domain::*;

impl Paint<'_, '_> {
    /// A byte `A0-FF` under ISO 8859-1: the character of the set GR invokes
    /// at the byte's 7-bit code (`[[source: ecma35]]` 8.3.2; `[[source:
    /// dec-std-070]]` 070-5 p. 5-140, `code = parv[0] & 0x7F` with the GR
    /// set).
    ///
    /// **With a 94-character set in GR, `A0` shows the error character and
    /// `FF` is ignored**: 070-3 §3.4.3, "The character code 10/0 in 8-bit
    /// environments is treated as an unassigned graphic character code, and
    /// will cause the error character ... to be displayed", and §3.4.4,
    /// "The character code 15/15 in 8-bit environments is treated as an
    /// unimplemented control code and shall be ignored" (note AE's AE8,
    /// decided). A 96-character set has a character at both (8.3.2).
    #[cold]
    #[inline(never)]
    pub(crate) fn gr_glyph(&mut self, b: StreamByte) {
        let set = self.cold.g[self.cold.gr.index()];
        let cp = match (set.is_96(), b.raw_byte()) {
            (false, 0xa0) => DEC_ERROR_CHARACTER,
            (false, 0xff) => return,
            _ => PackedCell::translate(b.seven_bit(), set),
        };
        glyph_any(self, cp, Pen::Kept(Received::of(set)));
    }

    /// The character after a single shift, from the slot the shift named,
    /// at its 7-bit code (`Node::SingleShift` says which bytes reach here).
    ///
    /// **`DEL` with a 94-character set in the slot consumes nothing**: it is
    /// no graphic character of the set, so "the single shift function is
    /// ignored and the received character is processed as if the single
    /// shift had not been received" (`[[source: dec-std-070]]` p. 3-81) --
    /// back to the ground, where GL decides. Under ISO 8859-1 that is `FF`
    /// too, which the ground hands to GR.
    ///
    /// The shift was a control, so the character starts a cluster of its own
    /// (`[[source: uax29]]` GB4), as one after any control does.
    #[cold]
    #[inline(never)]
    pub(crate) fn shifted(&mut self, b: StreamByte) -> ReadLen {
        let set = self.cold.g[self.cold.single.index()];
        let code = b.seven_bit();
        if code.raw_byte() == 0x7f && !set.is_96() {
            return ReadLen::ZERO;
        }
        self.cluster = ClusterBreak::after_control();
        glyph_any(
            self,
            PackedCell::translate(code, set),
            Pen::Kept(Received::of(set)),
        );
        ReadLen::ONE
    }

    /// `DEL` in the ground with a 96-character set in GL: the set's character
    /// at 7/15 (`[[source: ecma35]]` 8.3.1, "that set shall be invoked into
    /// positions 02/00 to 07/15"), under GL's pen as a GL code is.
    #[cold]
    #[inline(never)]
    pub(crate) fn gl_del(&mut self) {
        let cp = PackedCell::translate(StreamByte::DEL, self.charset);
        glyph_any(self, cp, Pen::Gl);
    }
}
