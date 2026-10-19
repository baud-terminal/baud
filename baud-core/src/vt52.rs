//! VT52, as a disjoint sub-graph rather than a mode test.
//!
//! **`ESC [` is not a CSI introducer here.** That is the whole reason this is
//! seven nodes instead of a `terminal_mode` branch inside the `Escape` arm: in
//! VT52 every byte parses differently, so the protocol belongs in the parser
//! state and not in a flag the hot loop has to read. `CLAUDE.md` records the
//! general form -- "a protocol is more nodes, not a runtime flag" -- from when
//! `ESC` itself and the UTF-8 continuations were made nodes, and both removed
//! a per-byte branch and were free.
//!
//! `[[source: ctlseqs]]` gives the command set; `baud_domain::Vt52` is the
//! parse, so what lives here is what each command does.

use crate::paint::Paint;
use baud_domain::*;

impl Paint<'_, '_> {
    /// Act on one VT52 command.
    ///
    /// Takes `&mut Node` because `ESC <` leaves the protocol, and leaving is a
    /// parser transition rather than a state change: the next byte has to be
    /// read by the ANSI graph. That is the mirror of `CSI ? 2 l`, which enters.
    #[cold]
    #[inline(never)]
    pub(crate) fn vt52(&mut self, c: Vt52, node: &mut crate::graph::Node) {
        match c {
            // **The page's edges, not the margins**: `[[source: dec-std-070]]`
            // VT52 Cursor Up and Cursor Down (pp. A-23, A-24) "will not move
            // above the first line of the display" and below its bottom
            // line, and VT52 has no margins, where ANSI `CUU`/`CUD` stop at
            // them (`Paint::up_in_margins`). DECOM's `confine` holds, as for
            // `Home` below, and `CUF`/`CUB` are the ANSI expressions at a
            // count of one.
            Vt52::CursorUp => self.set_row(self.confine(self.cy.up(Count::ONE))),
            Vt52::CursorDown => {
                let rows = self.rows;
                self.set_row(self.confine(self.cy.down(Count::ONE, rows)));
            }
            Vt52::CursorRight => {
                let c = self.confine_col(self.cx.col().forward(Count::ONE, self.cols));
                self.set_col(self.on_line(self.place(c)))
            }
            Vt52::CursorLeft => {
                let c = self.confine_col(self.cx.col().back(Count::ONE));
                self.set_col(self.place(c))
            }
            // `[[source: ctlseqs]]` maps `ESC F`/`ESC G` onto the DEC special
            // graphics set, which this core already has for `ESC ( 0` -- so
            // this is a designation into the active slot rather than a second
            // glyph table.
            Vt52::GraphicsOn => self.designate(Charset::DecGraphics),
            Vt52::GraphicsOff => self.designate(Charset::Ascii),
            // **DECOM's confinement holds here too**: `[[source: dec-std-070]]`
            // § A.5.5 leaves Origin Mode and the scrolling region "undefined
            // ... both when entering and exiting VT52 Mode", so no text
            // lets the cursor out, and a `CPR` after `ESC <` must agree.
            Vt52::Home => self.home(),
            Vt52::ReverseLineFeed => self.reverse_index(),
            Vt52::EraseToEndOfScreen => self.erase_display(EraseExtent::ToEnd),
            Vt52::EraseToEndOfLine => self.erase_line(EraseExtent::ToEnd),
            // Handled by the graph: `ESC Y` moves to `Vt52Row`, so this arm is
            // only reached if `Vt52::parse` and the graph ever disagree about
            // which byte introduces an address.
            Vt52::DirectAddress => {}
            // **The one reply that is neither a CSI nor a control string.**
            // `[[source: ctlseqs]]`: "ESC / Z Report (terminal to host)".
            Vt52::Identify => self.want_reply(Report::Vt52Identity),
            Vt52::ExitVt52 => {
                self.cold.terminal = TerminalMode::Ansi;
                *node = crate::graph::Node::AfterControl;
            }
            Vt52::KeypadApplication => self.cold.keypad = KeypadMode::Application,
            Vt52::KeypadNumeric => self.cold.keypad = KeypadMode::Numeric,
            // **The print functions, and they are counted rather than
            // silent.** `ESC ^`, `ESC _`, `ESC W`, `ESC X`, `ESC ]`, `ESC V`
            // drive a printer this core does not have. A byte that is not a
            // VT52 command at all lands here too, and both are the same
            // answer: consumed, not drawn, and a number saying how often.
            Vt52::Unsupported => self.vt52_unsupported += 1,
        }
    }

    /// `ESC F` / `ESC G`: designate into the active slot and invoke it.
    ///
    /// `[[source: ctlseqs]]` maps VT52 graphics mode onto the DEC special
    /// graphics set, which this core already has for `ESC ( 0` -- so this is a
    /// designation rather than a second glyph table. Going through `invoke`
    /// means the resolved set reaches `Paint::charset` the same way an ANSI
    /// designation does, and `SO`/`SI` keep working across the switch.
    fn designate(&mut self, c: Charset) {
        self.cold.g[self.cold.gl.index()] = c;
        self.invoke();
    }

    /// The column byte of `ESC Y <row> <col>`, which completes the address.
    ///
    /// **Both coordinates are biased by `0x20` and both clamp.** A value below
    /// `0x20` would be a negative coordinate, which no application sends, and
    /// one past the screen would address a cell that is not there.
    /// `Vt52Coord` does the clamping, so the two call sites cannot disagree
    /// about it.
    #[cold]
    #[inline(never)]
    pub(crate) fn vt52_address(&mut self, col: Vt52Coord) {
        let (r, c) = (self.cold.vt52_row.to_row(self.rows), col.to_col(self.cols));
        let c = self.place(self.confine_col(c));
        self.goto_at(self.confine(r), c);
    }
}
