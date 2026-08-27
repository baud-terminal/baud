use crate::*;

impl<'a> Term<'a> {
    /// Execute one VT52 escape sequence.
    ///
    /// `[[source: vt510-decanm]]` Table 5-1. This is a separate dispatch from
    /// `esc_fe` on purpose: the two protocols disagree about what the same
    /// bytes mean. `ESC D` is IND here and cursor-left there; `ESC H` is HTS
    /// here and home there. Folding them into one table would silently make one
    /// of the two wrong.
    pub(crate) fn vt52_dispatch(&mut self, b: StreamByte) {
        match Vt52::parse(b) {
            Vt52::CursorUp => self.cy = self.cy.up(Count::ONE),
            Vt52::CursorDown => self.cy = self.cy.down(Count::ONE, self.rows),
            Vt52::CursorRight => self.cx = self.cx.forward(Count::ONE, self.cols),
            Vt52::CursorLeft => self.cx = self.cx.back(Count::ONE),
            // VT52 reaches the line-drawing set directly rather than through a
            // G-slot designation; the cell flag is the same one either route.
            Vt52::GraphicsOn => {
                self.g[GSlot::G0.index()] = Charset::DecGraphics;
                self.refresh_flags();
            }
            Vt52::GraphicsOff => {
                self.g[GSlot::G0.index()] = Charset::Ascii;
                self.refresh_flags();
            }
            Vt52::Home => {
                self.cx = Col::ZERO;
                self.cy = Row::ZERO;
            }
            Vt52::ReverseLineFeed => self.reverse_index(),
            Vt52::EraseToEndOfScreen => self.erase_display(EraseExtent::ToEnd),
            Vt52::EraseToEndOfLine => self.erase_line(EraseExtent::ToEnd),
            // Two coordinate bytes follow; the state machine collects them.
            Vt52::DirectAddress => self.st = State::Vt52Row,
            Vt52::Identify => self.report(Report::Vt52Identity),
            Vt52::ExitVt52 => self.set_terminal_mode(TerminalMode::Ansi),
            Vt52::KeypadApplication => self.keypad = KeypadMode::Application,
            Vt52::KeypadNumeric => self.keypad = KeypadMode::Numeric,
            // The print functions -- `ESC ^ _ W X ] V`. There is no printer,
            // and accepting them silently is the failure this counter exists
            // to make visible.
            Vt52::Unsupported => self.unsupported.inc(),
        }
    }

    /// The column byte of `ESC Y row col`, with the row already collected.
    pub(crate) fn vt52_address(&mut self, row: Vt52Coord, col: StreamByte) {
        self.cy = row.to_row(self.rows);
        self.cx = Vt52Coord::from_wire(col).to_col(self.cols);
    }

    /// The ground state for the protocol currently in force.
    ///
    /// Read only when a sequence *ends*, never in the byte loop -- which is the
    /// point of having two ground states at all.
    #[inline(always)]
    pub(crate) fn ground(&self) -> State {
        match self.terminal_mode {
            TerminalMode::Ansi => State::Ground,
            TerminalMode::Vt52 => State::Vt52Ground,
        }
    }

    /// Enter or leave VT52, putting the parser in the matching ground state.
    pub(crate) fn set_terminal_mode(&mut self, m: TerminalMode) {
        self.terminal_mode = m;
        self.st = self.ground();
    }

    /// Is the parser speaking VT52 rather than ANSI? DECANM.
    pub fn terminal_mode(&self) -> TerminalMode {
        self.terminal_mode
    }
}
