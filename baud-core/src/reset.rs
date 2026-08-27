use crate::*;

impl<'a> Term<'a> {
    /// DECSTR. Modes, region, cursor and attributes to their defaults --
    /// **not** the screen contents. That is what makes it soft, and it is why
    /// an application uses it to recover a terminal without losing scrollback.
    pub(crate) fn soft_reset(&mut self) {
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
        self.style = StyleId::DEFAULT;
        self.style_raw = StyleWord::DEFAULT;
        self.flags = CellFlags::NONE;
        self.protect = CellFlags::NONE;
        self.keypad = KeypadMode::Numeric;
        self.set_terminal_mode(TerminalMode::Ansi);
        self.g = [Charset::Ascii; 4];
        self.gl = GSlot::G0;
        self.cursor_style = CursorStyle::BlinkingBlock;
        // **Not** `screen`, `alt_saved`, `mouse` or `mouse_encoding`.
        //
        // Those four used to be reset here, which meant a `CSI ! p` sent by a
        // full-screen application dropped it onto the primary screen mid-draw
        // and discarded the primary's saved contents with `alt_saved = None`.
        //
        // The alternate screen and mouse tracking are xterm extensions, so a
        // DEC manual's list of what DECSTR resets cannot mention them and
        // xterm is the authority. In `ReallyReset` its soft path does nothing
        // mouse-related at all, and `FromAlternate` sits in the `if (full)`
        // block -- alongside `ClearScreen`, which DECSTR must not do either.
        // RIS still resets all four; see `Term::reset`.
        //
        // The DEC-defined items above stay, including `saved`: DEC STD 070
        // has DECSTR set the saved cursor to home, and xterm resets it only
        // under RIS. Where the two disagree about a *DEC* function the DEC
        // manual wins; where the item is an xterm extension, xterm does.
        self.saved = None;
        self.scroll_top = Row::ZERO;
        self.scroll_bot = Row::from_index(self.rows.index() - 1);
        self.reset_side_margins();
        self.modes = Modes::default();
    }

    /// RIS. Everything back to power-on state, including the scrolling region
    /// and the modes -- a region left over across a reset is how an application
    /// ends up drawing into a window it did not ask for.
    pub(crate) fn reset(&mut self) {
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
        self.style = StyleId::DEFAULT;
        self.style_raw = StyleWord::DEFAULT;
        self.flags = CellFlags::NONE;
        self.protect = CellFlags::NONE;
        self.keypad = KeypadMode::Numeric;
        self.set_terminal_mode(TerminalMode::Ansi);
        self.mouse = MouseTracking::Off;
        self.mouse_encoding = MouseEncoding::X10;
        self.g = [Charset::Ascii; 4];
        self.gl = GSlot::G0;
        self.cursor_style = CursorStyle::BlinkingBlock;
        self.screen = Screen::Normal;
        self.alt_saved = None;
        self.saved = None;
        self.scroll_top = Row::ZERO;
        self.scroll_bot = Row::from_index(self.rows.index() - 1);
        self.reset_side_margins();
        self.modes = Modes::default();
        self.reset_tabs();
        self.clear_screen();
    }
}
